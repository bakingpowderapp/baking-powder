use super::*;

mod pass;
mod plan;
mod pool;
mod stream;

pub use pool::{Demote, Workers};
pub(super) use pass::{each_midi, each_runner, each_struck, forget_onsets, restart_rows, retime_onsets, run_rows};
use pass::run_twin;
pub(super) use plan::{Delay, Lags, Line, Plan, ReturnPlan, TrackPlan, shift};
pub(super) use pool::{Countdown, Lead, Phase, start_pool};
pub(super) use stream::{Printer, Stream};
use pass::{Pass, run_routed};

pub(super) type Rows = [Mutex<Option<Insert>>; RACK_ROWS];

const SMOOTHING: f32 = 0.25;

fn empty_rows() -> Rows {
    std::array::from_fn(|_| Mutex::new(None))
}

pub(super) fn bits(mut mask: u64) -> impl Iterator<Item = usize> {
    std::iter::from_fn(move || {
        let bit = (mask != 0).then(|| mask.trailing_zeros() as usize)?;
        mask &= mask - 1;
        Some(bit)
    })
}

fn smoothed(held: f32, sample: f32) -> f32 {
    if held > 0.0 { held + (sample - held) * SMOOTHING } else { sample }
}

#[derive(Clone, Copy)]
pub(super) struct Fader {
    from: f32,
    to: f32,
    over: usize,
    done: usize,
}

impl Fader {
    pub(super) const UNITY: Fader = Fader { from: 1.0, to: 1.0, over: 0, done: 0 };

    pub(super) fn set(&mut self, gain: f32) {
        self.sweep(gain, gain, 0);
    }

    pub(super) fn sweep(&mut self, from: f32, to: f32, frames: usize) {
        *self = Fader { from, to, over: frames, done: 0 };
    }

    pub(super) fn at(&self, frame: usize) -> f32 {
        let reached = self.done + frame;
        if reached < self.over { self.from + (self.to - self.from) * reached as f32 / self.over as f32 } else { self.to }
    }

    pub(super) fn advance(&mut self, frames: usize) {
        self.done = (self.done + frames).min(self.over);
    }
}

pub(crate) struct Strip {
    pub(super) l: Vec<f32>,
    pub(super) r: Vec<f32>,
    pub(super) pre_l: Vec<f32>,
    pub(super) pre_r: Vec<f32>,
    pub(super) gain: Fader,
    pub(super) heard: bool,
    pub(super) pours: bool,
    pub(super) unmuted: bool,
    pub(super) listened: bool,
    pub(super) doubled: bool,
    pub(super) group: Option<usize>,
    pub(super) members: u64,
    pub(super) inputs: u64,
    pub(super) cost: f32,
    pub(super) core_cost: f32,
    pub(super) sends: [f32; SEND_COUNT],
    pub(super) core_solo: Option<Alone>,
    pub(super) pass: u64,
    pub(super) plays: Vec<Play>,
    pub(super) out: Line,
    pub(super) pre: Line,
    pub(super) signal_l: Vec<f32>,
    pub(super) signal_r: Vec<f32>,
    pub(super) plan: Option<Box<TrackPlan>>,
    pub(super) twin: Option<Box<TwinLines>>,
}

pub(crate) struct TwinLines {
    l: Vec<f32>,
    r: Vec<f32>,
    pre_l: Vec<f32>,
    pre_r: Vec<f32>,
    aside_l: Vec<f32>,
    aside_r: Vec<f32>,
    out: Line,
    pre: Line,
}

impl TwinLines {
    pub(super) fn new(max_frames: usize, cap: usize) -> TwinLines {
        TwinLines { l: vec![0.0; max_frames], r: vec![0.0; max_frames], pre_l: vec![0.0; max_frames], pre_r: vec![0.0; max_frames], aside_l: vec![0.0; max_frames], aside_r: vec![0.0; max_frames], out: Line::new(cap + max_frames), pre: Line::new(cap + max_frames) }
    }

    fn holds(&self, n: usize) -> bool {
        [&self.l, &self.r, &self.pre_l, &self.pre_r, &self.aside_l, &self.aside_r].iter().all(|buffer| buffer.len() >= n)
    }

    pub(super) fn clear(&mut self) {
        self.out.clear();
        self.pre.clear();
    }
}

pub(super) struct Signal<'a> {
    l: &'a [f32],
    r: &'a [f32],
    pre_l: &'a [f32],
    pre_r: &'a [f32],
    out: &'a Line,
    pre: &'a Line,
}

impl Strip {
    pub(super) const fn empty() -> Strip {
        Strip { l: Vec::new(), r: Vec::new(), pre_l: Vec::new(), pre_r: Vec::new(), gain: Fader::UNITY, heard: true, pours: true, unmuted: true, listened: false, doubled: false, group: None, members: 0, inputs: 0, cost: 0.0, core_cost: 0.0, sends: [0.0; SEND_COUNT], core_solo: None, pass: 0, plays: Vec::new(), out: Line::empty(), pre: Line::empty(), signal_l: Vec::new(), signal_r: Vec::new(), plan: None, twin: None }
    }

    pub(super) fn signal(&self) -> Signal<'_> {
        match self.twin.as_deref().filter(|_| self.doubled) {
            Some(twin) => Signal { l: &twin.l, r: &twin.r, pre_l: &twin.pre_l, pre_r: &twin.pre_r, out: &twin.out, pre: &twin.pre },
            None => Signal { l: &self.l, r: &self.r, pre_l: &self.pre_l, pre_r: &self.pre_r, out: &self.out, pre: &self.pre },
        }
    }

    pub(super) fn shift(&self) -> (isize, isize) {
        self.plan.as_deref().map_or((0, 0), |plan| plan.shift)
    }

    pub(super) fn charge(&mut self, spent: Duration, cores: u64, frames: usize) {
        let per_frame = |nanos: f64| (nanos / frames.max(1) as f64) as f32;
        self.cost = smoothed(self.cost, per_frame(spent.as_nanos() as f64));
        self.core_cost = smoothed(self.core_cost, per_frame(cores as f64));
    }

    pub(super) fn fresh(max_frames: usize, cap: usize) -> Strip {
        Strip { l: vec![0.0; max_frames], r: vec![0.0; max_frames], pre_l: vec![0.0; max_frames], pre_r: vec![0.0; max_frames], out: Line::new(cap + max_frames), pre: Line::new(cap + max_frames), signal_l: vec![0.0; max_frames], signal_r: vec![0.0; max_frames], ..Strip::empty() }
    }
}

pub(super) struct Return {
    pub(super) l: Vec<f32>,
    pub(super) r: Vec<f32>,
    pub(super) signal_l: Vec<f32>,
    pub(super) signal_r: Vec<f32>,
    pub(super) gain: Fader,
    pub(super) fed: u64,
    pub(super) pass: u64,
    pub(super) line: Line,
    pub(super) plan: ReturnPlan,
}

impl Return {
    fn fresh(max_frames: usize, cap: usize) -> Return {
        Return { l: vec![0.0; max_frames], r: vec![0.0; max_frames], signal_l: vec![0.0; max_frames], signal_r: vec![0.0; max_frames], gain: Fader::UNITY, fed: 0, pass: 0, line: Line::new(cap + max_frames), plan: ReturnPlan::default() }
    }
}

pub(super) struct TrackCell {
    pub(super) rows: Rows,
    pub(super) strip: RwLock<Strip>,
    pub(super) unrendered: Countdown,
    pub(super) core_nanos: AtomicU64,
}

impl TrackCell {
    pub(super) fn shift(&self) -> (isize, isize) {
        peek(&self.strip).map_or((0, 0), |strip| strip.shift())
    }
}

pub(super) struct SendCell {
    pub(super) rows: Rows,
    pub(super) strip: Mutex<Return>,
}

#[derive(Clone, Copy)]
pub(super) struct Segment {
    pub(super) frame: i64,
    pub(super) beat: f64,
    pub(super) bpm: f64,
    pub(super) frames_per_beat: f64,
    pub(super) playing: bool,
}

impl Segment {
    pub(super) const fn stopped(frame: i64) -> Segment {
        Segment { frame, beat: 0.0, bpm: 0.0, frames_per_beat: 0.0, playing: false }
    }

    pub(super) fn beat_at(&self, frame: i64) -> f64 {
        self.beat + (frame - self.frame) as f64 / self.frames_per_beat
    }
}

#[derive(Clone, Copy)]
pub(super) struct Mark {
    newest: usize,
    count: usize,
}

impl Mark {
    pub(super) const EMPTY: Mark = Mark { newest: 0, count: 0 };

    pub(super) fn forget(&mut self) {
        self.count = 0;
    }
}

pub(super) struct Segments {
    slots: Vec<[AtomicU64; 4]>,
}

impl Segments {
    fn covering(frames: usize) -> Segments {
        Segments { slots: (0..=frames).map(|_| std::array::from_fn(|_| AtomicU64::new(0))).collect() }
    }

    fn slot(&self, mark: Mark, age: usize) -> usize {
        let len = self.slots.len();
        (mark.newest + len - age % len) % len
    }

    fn frame(&self, slot: usize) -> i64 {
        self.slots[slot][0].load(Ordering::Relaxed) as i64
    }

    fn load(&self, slot: usize) -> Segment {
        let [frame, beat, bpm, frames_per_beat] = self.slots[slot].each_ref().map(|word| word.load(Ordering::Relaxed));
        let frames_per_beat = f64::from_bits(frames_per_beat);
        Segment { frame: frame as i64, beat: f64::from_bits(beat), bpm: f64::from_bits(bpm), frames_per_beat, playing: frames_per_beat > 0.0 }
    }

    pub(super) fn write(&self, mark: &mut Mark, segment: Segment) {
        if mark.count == 0 || self.frame(mark.newest) != segment.frame {
            mark.newest = (mark.newest + 1) % self.slots.len();
            mark.count = (mark.count + 1).min(self.slots.len());
        }
        let frames_per_beat = if segment.playing { segment.frames_per_beat } else { 0.0 };
        let words = [segment.frame as u64, segment.beat.to_bits(), segment.bpm.to_bits(), frames_per_beat.to_bits()];
        for (word, value) in self.slots[mark.newest].iter().zip(words) {
            word.store(value, Ordering::Relaxed);
        }
    }

    pub(super) fn at(&self, mark: Mark, frame: i64) -> Segment {
        let Some(oldest) = mark.count.checked_sub(1) else {
            return Segment::stopped(frame);
        };
        let slot = (0..oldest).map(|age| self.slot(mark, age)).find(|&slot| self.frame(slot) <= frame);
        self.load(slot.unwrap_or_else(|| self.slot(mark, oldest)))
    }

    pub(super) fn after(&self, mark: Mark, frame: i64) -> Option<Segment> {
        (0..mark.count.saturating_sub(1)).map(|age| self.slot(mark, age)).take_while(|&slot| self.frame(slot) > frame).last().map(|slot| self.load(slot))
    }
}

#[derive(Clone, Copy)]
pub(super) struct Dispatch {
    pub(super) frames: usize,
    pub(super) tracks: usize,
    pub(super) cores: usize,
    pub(super) generation: u64,
    pub(super) now: i64,
    pub(super) frames_per_beat: f64,
    pub(super) transport: Option<Transport>,
    pub(super) pour: Option<Pour>,
    pub(super) tail: Option<(f64, Ramp)>,
    pub(super) clock: Clock,
    pub(super) mark: Mark,
    pub(super) targets: [Target; SCOPES],
    pub(super) deadline: Instant,
    pub(super) line: Segment,
    pub(super) span: Option<(i64, i64)>,
    pub(super) window: Option<Window>,
    pub(super) measured: Option<(i64, i64)>,
    pub(super) timed: bool,
    pub(super) batches: u64,
}

#[derive(Clone, Copy)]
pub(super) struct Window {
    pub(super) from: i64,
    pub(super) to: i64,
    pub(super) moved: bool,
}

impl Window {
    pub(super) fn of((from, to): (i64, i64), (soonest, latest): (isize, isize)) -> Window {
        Window { from: from - latest as i64, to: to - soonest as i64, moved: (soonest, latest) != (0, 0) }
    }

    pub(super) fn waits(self, now: i64, frames: usize) -> bool {
        now + frames as i64 <= self.from
    }
}

impl Dispatch {
    pub(super) fn behind(&self, upstream: usize, segments: &Segments) -> Clock {
        let frame = self.now - upstream as i64;
        let segment = match self.window {
            Some(window) if frame >= window.to => Segment::stopped(window.to),
            Some(_) => self.line,
            None => segments.at(self.mark, frame),
        };
        if segment.playing {
            Clock { bpm: segment.bpm, beat: Some(Beat { anchor: segment.beat, since: frame - segment.frame, frames_per_beat: segment.frames_per_beat }), ..self.clock }
        } else {
            Clock { beat: None, ..self.clock }
        }
    }

    pub(super) fn on_track<R>(&self, shift: impl FnOnce() -> (isize, isize), job: impl FnOnce(&Dispatch) -> R) -> Option<R> {
        let Some(span) = self.span else {
            return Some(job(self));
        };
        let window = Window::of(span, shift());
        if window.waits(self.now, self.frames) {
            return None;
        }
        if !window.moved {
            return Some(job(self));
        }
        let (line, playing) = (self.line, self.now < window.to);
        let beat = playing.then_some(Beat { anchor: line.beat, since: self.now - line.frame, frames_per_beat: line.frames_per_beat });
        let transport = playing.then(|| Transport { beat: line.beat_at(self.now) });
        Some(job(&Dispatch { transport, pour: None, tail: None, clock: Clock { beat, ..self.clock }, window: Some(window), ..*self }))
    }

    pub(super) fn idle() -> Dispatch {
        Dispatch {
            frames: 0,
            tracks: 0,
            cores: 0,
            generation: 0,
            now: 0,
            frames_per_beat: 0.0,
            transport: None,
            pour: None,
            tail: None,
            clock: Clock { sample_rate: 0.0, bpm: 0.0, beat: None },
            mark: Mark::EMPTY,
            targets: [Target::NONE; SCOPES],
            deadline: Instant::now(),
            line: Segment::stopped(0),
            span: None,
            window: None,
            measured: None,
            timed: false,
            batches: 0,
        }
    }
}

pub(super) struct Table {
    pub(super) tracks: Vec<TrackCell>,
    pub(super) sends: Vec<SendCell>,
    pub(super) master: Rows,
    pub(super) block: Mutex<Dispatch>,
    pub(super) generation: AtomicU64,
    pub(super) mixed: AtomicU64,
    pub(super) stop: AtomicBool,
    pub(super) resting: AtomicBool,
    pub(super) key: AtomicU16,
    pub(super) cores: Phase,
    pub(super) strips: Phase,
    pub(super) returns: Phase,
    pub(super) meters: Arc<Meters>,
    pub(super) workers: Mutex<Vec<Thread>>,
    pub(super) busy: AtomicUsize,
    pub(super) order: Vec<AtomicUsize>,
    pub(super) core_rows: Vec<AtomicUsize>,
    pub(super) segments: Segments,
}

impl Table {
    pub(super) fn new(max_frames: usize, cap: usize, meters: Arc<Meters>) -> Table {
        Table {
            tracks: (0..TRACKS).map(|_| TrackCell { rows: empty_rows(), strip: RwLock::new(Strip::empty()), unrendered: Countdown::new(), core_nanos: AtomicU64::new(0) }).collect(),
            sends: (0..SEND_COUNT).map(|_| SendCell { rows: empty_rows(), strip: Mutex::new(Return::fresh(max_frames, cap)) }).collect(),
            master: empty_rows(),
            block: Mutex::new(Dispatch::idle()),
            generation: AtomicU64::new(0),
            mixed: AtomicU64::new(0),
            stop: AtomicBool::new(false),
            resting: AtomicBool::new(true),
            key: AtomicU16::new(EVERY_CLASS),
            cores: Phase::new(),
            strips: Phase::new(),
            returns: Phase::new(),
            meters,
            workers: Mutex::new(Vec::new()),
            busy: AtomicUsize::new(0),
            order: (0..TRACKS).map(AtomicUsize::new).collect(),
            core_rows: (0..TRACKS * RACK_ROWS).map(|_| AtomicUsize::new(0)).collect(),
            segments: Segments::covering(cap + max_frames),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Ramp {
    Up(f64),
    Down { over: usize, done: usize },
}

impl Ramp {
    fn at(self, frame: i64) -> f32 {
        match self {
            Ramp::Up(since) => ((since + frame as f64 + 1.0) / f64::from(FADE)) as f32,
            Ramp::Down { over, done } => (over as i64 - done as i64 - frame) as f32 / over as f32,
        }
    }
}

fn shaped(regions: &[Region], smear: f64, frame: f64) -> f32 {
    if regions.is_empty() {
        return 1.0;
    }
    let (lo, hi) = (frame - smear / 2.0, frame + smear / 2.0);
    let mut kept = smear;
    for region in regions {
        let overlap = region.to.min(hi) - region.from.max(lo);
        if overlap > 0.0 {
            kept -= overlap * (1.0 - f64::from(region.gain));
        }
    }
    (kept / smear) as f32
}

fn pour_core(layer: &Layer, l: &[f32], r: &[f32], into_l: &mut [f32], into_r: &mut [f32]) {
    let n = into_l.len().min(into_r.len());
    let level = layer.level as f32;
    for i in 0..n {
        into_l[i] += l[i] * level;
        into_r[i] += r[i] * level;
    }
}

fn faded(l: &mut [f32], r: &mut [f32], gain: &mut Fader) -> (f32, f32) {
    let n = l.len().min(r.len());
    let (mut peak_l, mut peak_r) = (0.0f32, 0.0f32);
    for i in 0..n {
        let level = gain.at(i);
        l[i] = finite(l[i] * level);
        r[i] = finite(r[i] * level);
        peak_l = peak_l.max(l[i].abs());
        peak_r = peak_r.max(r[i].abs());
    }
    gain.advance(n);
    (peak_l, peak_r)
}

pub(super) fn pour(plays: &[Play], beat: f64, ramp: Ramp, frames_per_beat: f64, sample_rate: f64, l: &mut [f32], r: &mut [f32]) {
    let n = l.len().min(r.len()) as i64;
    for play in plays {
        let first = ((play.start - beat) * frames_per_beat).ceil() as i64;
        let last = ((play.end - beat) * frames_per_beat).ceil() as i64;
        let (from, to) = (first.max(0), last.min(n));
        if from >= to {
            continue;
        }
        let rate = play.wave.rate / sample_rate;
        let step = rate * play.ratio;
        let smear = Span::RAMP * play.wave.rate;
        let origin = (beat - play.anchor) * frames_per_beat;
        let stop = match &play.warp {
            Some(warp) => play.stop * warp.ratio / rate - origin,
            None => play.stop / step - origin,
        };
        let mut cursor = play.warp.as_ref().map_or(0, |warp| warp.cursor_at((origin + from as f64) * rate));
        for i in from..to {
            let left_to_play = stop - i as f64;
            if left_to_play <= 0.0 {
                break;
            }
            let elapsed = origin + i as f64;
            let (frame, (left, right)) = match &play.warp {
                Some(warp) => {
                    let d = elapsed * rate;
                    (d / warp.ratio, warp.sample(&play.wave, d, step, play.ratio, &mut cursor))
                }
                None => {
                    let frame = elapsed * step;
                    (frame, play.wave.at(frame, step))
                }
            };
            let gain = ((i - first + 1).min(last - i) as f32 / FADE).min(ramp.at(i)).min(left_to_play as f32 / FADE).clamp(0.0, 1.0) * play.gain * shaped(&play.regions, smear, frame) * play.fades.gain(frame / play.wave.rate) as f32;
            let at = i as usize;
            l[at] += left * gain;
            r[at] += right * gain;
        }
    }
}

fn pour_window(plays: &[Play], window: Window, d: &Dispatch, l: &mut [f32], r: &mut [f32]) {
    let n = l.len().min(r.len()) as i64;
    let (first, last) = ((window.from - d.now).clamp(0, n), (window.to - d.now).clamp(0, n));
    if first >= last {
        return;
    }
    let beat = d.line.beat_at(d.now + first);
    let since = ((beat - d.line.beat_at(window.from)) * d.frames_per_beat).max(0.0);
    let (first, last) = (first as usize, last as usize);
    pour(plays, beat, Ramp::Up(since), d.frames_per_beat, d.clock.sample_rate, &mut l[first..last], &mut r[first..last]);
}

fn won<G>(tried: std::sync::TryLockResult<G>) -> Option<G> {
    match tried {
        Ok(guard) => Some(guard),
        Err(std::sync::TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
        Err(std::sync::TryLockError::WouldBlock) => None,
    }
}

pub(super) fn grab<T>(mutex: &Mutex<T>) -> Option<MutexGuard<'_, T>> {
    won(mutex.try_lock())
}

pub(super) fn peek<T>(lock: &RwLock<T>) -> Option<RwLockReadGuard<'_, T>> {
    won(lock.try_read())
}

pub(super) fn hold<T>(lock: &RwLock<T>) -> Option<RwLockWriteGuard<'_, T>> {
    won(lock.try_write())
}

pub(super) fn tap_for<'a>(meters: &'a Meters, targets: &[Target; SCOPES], target: Target) -> Option<&'a Tap> {
    targets.iter().position(|aimed| *aimed == target).map(|scope| &meters.taps[scope])
}

fn hear(ear: &Mutex<Option<Ear>>, (from, to): (i64, i64), now: i64, l: &[f32], r: &[f32]) {
    let edge = |frame: i64| (frame - now).clamp(0, l.len().min(r.len()) as i64) as usize;
    let (from, to) = (edge(from), edge(to));
    if from < to && let Some(ear) = ear.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_mut() {
        ear.hear(&l[from..to], &r[from..to]);
    }
}

impl Table {
    pub(super) fn warm(&self) {
        drop(self.workers.lock());
        for cell in &self.tracks {
            for row in &cell.rows {
                drop(row.lock());
            }
            drop(cell.strip.write());
        }
        for cell in &self.sends {
            for row in &cell.rows {
                drop(row.lock());
            }
            drop(cell.strip.lock());
        }
        for row in &self.master {
            drop(row.lock());
        }
        drop(self.block.lock());
    }

    pub(super) fn rows(&self, host: Host) -> Option<&Rows> {
        match host {
            Host::Track(track) => self.tracks.get(track).map(|cell| &cell.rows),
            Host::Send(send) => self.sends.get(send).map(|cell| &cell.rows),
            Host::Master => Some(&self.master),
        }
    }

    pub(super) fn len(rows: &Rows) -> usize {
        rows.iter().position(|row| grab(row).is_none_or(|row| row.is_none())).unwrap_or(RACK_ROWS)
    }

    fn before(&self, source: usize, track: usize, d: &Dispatch) -> bool {
        let position = |track: usize| self.order[..d.tracks.min(self.order.len())].iter().position(|slot| slot.load(Ordering::Relaxed) == track);
        source != track && source < d.tracks && position(source).zip(position(track)).is_some_and(|(earlier, mine)| earlier < mine)
    }

    fn pour_members(&self, members: u64, index: usize, l: &mut [f32], r: &mut [f32], mut twin: Option<&mut TwinLines>, d: &Dispatch) {
        let n = l.len();
        for member in (0..d.tracks).filter(|&member| members & (1 << member) != 0 && self.before(member, index, d)) {
            let Some(cell) = self.tracks.get(member).filter(|cell| self.source_ready(cell, d)) else {
                continue;
            };
            let Some(source) = peek(&cell.strip).filter(|source| source.l.len() >= n) else {
                continue;
            };
            let offset = source.plan.as_ref().map_or(0, |plan| plan.to_group);
            if source.pours {
                source.out.add(offset, 1.0, l, r);
            }
            if let Some(twin) = twin.as_deref_mut().filter(|_| source.unmuted) {
                source.signal().out.add(offset, 1.0, &mut twin.l[..n], &mut twin.r[..n]);
            }
        }
    }

    fn feed_signals(&self, cell: &TrackCell, strip: &mut Strip, index: usize, d: &Dispatch) {
        let n = d.frames;
        let Strip { signal_l, signal_r, plan, .. } = strip;
        for (position, row) in cell.rows.iter().enumerate() {
            let Some(mut row) = grab(row) else {
                continue;
            };
            let Some(insert) = row.as_mut() else {
                break;
            };
            let Insert::Effect { slot, unit, twin, .. } = insert else {
                continue;
            };
            let (source, pre) = slot.source().unzip();
            let fed = source.filter(|&source| self.before(source, index, d)).and_then(|source| self.tracks.get(source)).filter(|source| self.source_ready(source, d)).and_then(|source| peek(&source.strip)).filter(|source| source.l.len() >= n && source.pre_l.len() >= n);
            let offset = plan.as_deref().and_then(|plan| plan.rows.get(position)).map_or(0, |row| row.signal);
            let signal = match (fed.as_deref().map(Strip::signal), pre == Some(true)) {
                (Some(signal), true) if offset == 0 => Some((&signal.pre_l[..n], &signal.pre_r[..n])),
                (Some(signal), false) if offset == 0 => Some((&signal.l[..n], &signal.r[..n])),
                (Some(signal), pre) => {
                    (if pre { signal.pre } else { signal.out }).read(offset, &mut signal_l[..n], &mut signal_r[..n]);
                    Some((&signal_l[..n], &signal_r[..n]))
                }
                (None, _) => None,
            };
            for unit in std::iter::once(unit).chain(twin.as_deref_mut().map(|twin| &mut twin.unit)) {
                match signal {
                    Some((l, r)) => unit.feed(l, r),
                    None => unit.starve(),
                }
            }
        }
    }

    pub(super) fn feed_mixed(&self, rows: &Rows, lags: &Lags, signal_l: &mut [f32], signal_r: &mut [f32], d: &Dispatch) {
        let n = d.frames.min(signal_l.len()).min(signal_r.len());
        for (row, lag) in rows.iter().zip(lags) {
            let Some(mut row) = grab(row) else {
                continue;
            };
            let Some(insert) = row.as_mut() else {
                break;
            };
            let Insert::Effect { slot, unit, .. } = insert else {
                continue;
            };
            let fed = slot.source().filter(|&(source, _)| source < d.tracks).and_then(|(source, pre)| Some((peek(&self.tracks.get(source)?.strip)?, pre))).filter(|(source, _)| source.pass == d.generation && source.l.len() >= n);
            match fed {
                Some((source, pre)) => {
                    let signal = source.signal();
                    (if pre { signal.pre } else { signal.out }).read(lag.signal, &mut signal_l[..n], &mut signal_r[..n]);
                    unit.feed(&signal_l[..n], &signal_r[..n]);
                }
                None => unit.starve(),
            }
        }
    }

    fn capture_pre(cell: &TrackCell, pre_l: &mut [f32], pre_r: &mut [f32], source_l: &[f32], source_r: &[f32], aside_l: &mut [f32], aside_r: &mut [f32], core_solo: Option<Alone>, mut plan: Option<&mut TrackPlan>, twin: bool, d: &Dispatch) {
        let n = pre_l.len();
        pre_l.copy_from_slice(&source_l[..n]);
        pre_r.copy_from_slice(&source_r[..n]);
        for (position, row) in cell.rows.iter().enumerate() {
            let Some(row) = grab(row) else {
                continue;
            };
            let Some(insert) = row.as_ref() else {
                break;
            };
            let Insert::Core { layer, l, r, pass, .. } = insert else {
                continue;
            };
            let sounding = *pass == d.generation && core_solo.is_none_or(|solo| solo.holds(position));
            let line = plan.as_deref_mut().and_then(|plan| plan.rows.get_mut(position)).map(|row| if twin { &mut row.twin_pre } else { &mut row.pre }).filter(|line| line.offset > 0);
            match line {
                Some(line) => {
                    let (aside_l, aside_r) = (&mut aside_l[..n], &mut aside_r[..n]);
                    aside_l.fill(0.0);
                    aside_r.fill(0.0);
                    if sounding {
                        pour_core(layer, l, r, aside_l, aside_r);
                    }
                    line.apply(aside_l, aside_r);
                    for i in 0..n {
                        pre_l[i] += aside_l[i];
                        pre_r[i] += aside_r[i];
                    }
                }
                None if sounding => pour_core(layer, l, r, pre_l, pre_r),
                None => {}
            }
        }
    }

    fn strip_job(&self, cell: &TrackCell, strip: &mut Strip, index: usize, d: &Dispatch) {
        let n = d.frames;
        if strip.l.len() < n || strip.pre_l.len() < n || strip.signal_l.len() < n {
            strip.pass = d.generation;
            return;
        }
        let shift = strip.shift();
        if d.on_track(|| shift, |d| self.play_strip(cell, strip, index, d)).is_none() {
            for buffer in [&mut strip.l, &mut strip.r, &mut strip.pre_l, &mut strip.pre_r] {
                buffer[..n].fill(0.0);
            }
            if let Some(twin) = strip.twin.as_deref_mut() {
                for buffer in [&mut twin.l, &mut twin.r, &mut twin.pre_l, &mut twin.pre_r] {
                    buffer.iter_mut().take(n).for_each(|sample| *sample = 0.0);
                }
            }
            strip.doubled = false;
            strip.pass = d.generation;
        }
    }

    fn play_strip(&self, cell: &TrackCell, strip: &mut Strip, index: usize, d: &Dispatch) {
        let meters = &self.meters;
        let n = d.frames;
        self.feed_signals(cell, strip, index, d);
        let core_solo = strip.core_solo;
        let Strip { l, r, pre_l, pre_r, signal_l, signal_r, out, pre, plan, plays, members, gain, heard, listened, doubled, twin, pass: rendered, .. } = strip;
        let (l, r) = (&mut l[..n], &mut r[..n]);
        let (pre_l, pre_r) = (&mut pre_l[..n], &mut pre_r[..n]);
        l.fill(0.0);
        r.fill(0.0);
        if let Some(window) = d.window {
            pour_window(&plays[..], window, d, l, r);
        }
        if let Some(live) = d.pour {
            pour(&plays[..], live.beat, Ramp::Up(((live.beat - live.origin) * d.frames_per_beat).max(0.0)), d.frames_per_beat, d.clock.sample_rate, l, r);
        }
        if let Some((tail, ramp)) = d.tail {
            pour(&plays[..], tail, ramp, d.frames_per_beat, d.clock.sample_rate, l, r);
        }
        let mut twin = twin.as_deref_mut().filter(|twin| *listened && twin.holds(n));
        if let Some(twin) = twin.as_deref_mut() {
            twin.l[..n].copy_from_slice(l);
            twin.r[..n].copy_from_slice(r);
        }
        if *members != 0 {
            self.pour_members(*members, index, l, r, twin.as_deref_mut(), d);
        }
        Table::capture_pre(cell, pre_l, pre_r, l, r, &mut signal_l[..n], &mut signal_r[..n], core_solo, plan.as_deref_mut(), false, d);
        if let Some(TwinLines { l, r, pre_l, pre_r, .. }) = twin.as_deref_mut() {
            Table::capture_pre(cell, &mut pre_l[..n], &mut pre_r[..n], &l[..n], &r[..n], &mut signal_l[..n], &mut signal_r[..n], None, plan.as_deref_mut(), true, d);
        }
        let mut plan = plan.as_deref_mut();
        let shift = plan.as_deref().map_or((0, 0), |plan| plan.shift);
        let latency = plan.as_deref().map_or(0, |plan| plan.latency) as i64;
        let measured = d.measured.map(|span| Window::of(span, shift)).map(|window| (window.from, window.to));
        let pass = Pass { meters, d, host: Host::Track(index), solo: core_solo, segments: &self.segments, measured };
        run_routed(&cell.rows, &pass, l, r, plan.as_deref_mut());
        let mut fader = *gain;
        let (peak_l, peak_r) = faded(l, r, gain);
        if *heard && let Some(meter) = meters.tracks.get(index) {
            meter.raise(peak_l, peak_r);
            if let Some((from, to)) = measured
                && let Some(ear) = meters.track_ears.get(index)
            {
                hear(ear, (from + latency, to + latency), d.now, l, r);
            }
        }
        out.push(l, r);
        pre.push(pre_l, pre_r);
        *doubled = match twin {
            Some(TwinLines { l, r, pre_l, pre_r, aside_l, aside_r, out, pre }) => {
                let (l, r) = (&mut l[..n], &mut r[..n]);
                let whole = run_twin(&cell.rows, d, &self.segments, measured, l, r, (&mut aside_l[..n], &mut aside_r[..n]), plan);
                faded(l, r, &mut fader);
                out.push(l, r);
                pre.push(&pre_l[..n], &pre_r[..n]);
                whole
            }
            None => false,
        };
        *rendered = d.generation;
    }

    pub(super) fn return_job(&self, cell: &SendCell, ret: &mut Return, index: usize, d: &Dispatch) {
        let n = d.frames;
        let meters = &self.meters;
        if ret.l.len() < n || ret.signal_l.len() < n {
            return;
        }
        let fed = ret.fed == d.generation;
        if !fed {
            ret.l[..n].fill(0.0);
            ret.r[..n].fill(0.0);
        }
        let measured = d.measured.map(|(from, to)| (from - ret.plan.shift as i64, to - ret.plan.shift as i64));
        if fed || grab(&cell.rows[0]).is_none_or(|row| row.is_some()) {
            let Return { l, r, signal_l, signal_r, plan, gain, .. } = ret;
            let (l, r) = (&mut l[..n], &mut r[..n]);
            self.feed_mixed(&cell.rows, &plan.rows, signal_l, signal_r, d);
            run_rows(&cell.rows, Host::Send(index), l, r, meters, d, &plan.rows, &self.segments, measured);
            let (peak_l, peak_r) = faded(l, r, gain);
            meters.sends[index].raise(peak_l, peak_r);
        }
        if let Some((from, to)) = measured {
            let latency = ret.plan.latency as i64;
            hear(&meters.send_ears[index], (from + latency, to + latency), d.now, &ret.l[..n], &ret.r[..n]);
        }
        ret.line.push(&ret.l[..n], &ret.r[..n]);
        ret.pass = d.generation;
    }
}

use crate::draw::{DrawList, Rect, Tone};
use crate::dsp::fft::Fft;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

pub const SCOPES: usize = 2;
pub const CURVE: usize = 75;
pub const CURVE_LOW_HZ: f64 = 30.0;
pub const CURVE_PER_OCTAVE: f64 = 8.0;
pub const CURVE_FULL_DB: f32 = 32.0;
const CURVE_WORDS: usize = CURVE.div_ceil(8);
const CELLS: usize = 512;
const WINDOW_LOG2: u32 = 12;
const WINDOW: usize = 1 << WINDOW_LOG2;
const BINS: usize = WINDOW / 2;
const RING: usize = 4 * WINDOW;
const REST_DB: f32 = -120.0;
const FLOOR_DB: f32 = -80.0;
const CEILING_DB: f32 = 6.0;
const QUANTUM: i32 = 2;
const LO_HZ: f64 = 20.0;
const HI_HZ: f64 = 20_000.0;
const TILT_HZ: f64 = 1000.0;
const TILT_DB_PER_OCTAVE: f64 = 3.0;
const HANN_SCALE: f64 = 0.816_496_580_927_726;
const MAGNITUDE_FLOOR: f64 = 1e-12;
const PEAK_SEED_DB: f32 = -160.0;
const SILENCE: f32 = 1e-7;
const ATTACK_SECONDS: f32 = 0.015;
const RELEASE_SECONDS: f32 = 0.5;
const STEP_MIN_SECONDS: f32 = 0.001;
const STEP_MAX_SECONDS: f32 = 0.5;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    Insert,
    Output,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Target(u64);

impl Target {
    pub const NONE: Target = Target(u64::MAX);

    pub const fn keyed(key: u64) -> Target {
        Target(key)
    }
}

pub struct Tap {
    aimed: AtomicU64,
    serving: AtomicU64,
    written: AtomicU64,
    pre: Vec<AtomicU32>,
    post: Vec<AtomicU32>,
    curve: [AtomicU64; CURVE_WORDS],
}

pub fn curve_hz(cell: usize) -> f64 {
    CURVE_LOW_HZ * 2f64.powf(cell as f64 / CURVE_PER_OCTAVE)
}

fn ring() -> Vec<AtomicU32> {
    (0..RING).map(|_| AtomicU32::new(0)).collect()
}

fn record(ring: &[AtomicU32], from: u64, samples: impl Iterator<Item = f32>) {
    for (i, sample) in samples.enumerate() {
        ring[(from as usize + i) % RING].store(sample.to_bits(), Ordering::Relaxed);
    }
}

fn mono(l: &[f32], r: &[f32]) -> impl Iterator<Item = f32> {
    l.iter().zip(r).map(|(l, r)| 0.5 * (l + r))
}

impl Default for Tap {
    fn default() -> Tap {
        Tap::new()
    }
}

impl Tap {
    pub fn new() -> Tap {
        Tap { aimed: AtomicU64::new(Target::NONE.0), serving: AtomicU64::new(Target::NONE.0), written: AtomicU64::new(0), pre: ring(), post: ring(), curve: std::array::from_fn(|_| AtomicU64::new(0)) }
    }

    pub fn aim(&self, target: Target) {
        self.aimed.store(target.0, Ordering::Relaxed);
    }

    pub fn aimed(&self) -> Target {
        Target(self.aimed.load(Ordering::Relaxed))
    }

    pub fn serve(&self, target: Target) {
        self.written.store(0, Ordering::Relaxed);
        for word in &self.curve {
            word.store(0, Ordering::Relaxed);
        }
        self.serving.store(target.0, Ordering::Release);
    }

    pub fn write_curve(&self, cuts: &[f32; CURVE]) {
        for (word, cells) in self.curve.iter().zip(cuts.chunks(8)) {
            let bits = cells.iter().fold(0u64, |bits, cut| (bits << 8) | u64::from((cut.clamp(0.0, CURVE_FULL_DB) / CURVE_FULL_DB * f32::from(u8::MAX)).round() as u8));
            word.store(bits, Ordering::Relaxed);
        }
    }

    pub fn read_curve(&self) -> [f32; CURVE] {
        let mut cuts = [0.0; CURVE];
        for (word, cells) in self.curve.iter().zip(cuts.chunks_mut(8)) {
            let bits = word.load(Ordering::Relaxed);
            let count = cells.len();
            for (i, cut) in cells.iter_mut().enumerate() {
                *cut = f32::from((bits >> (8 * (count - 1 - i))) as u8) / f32::from(u8::MAX) * CURVE_FULL_DB;
            }
        }
        cuts
    }

    pub fn write_pre(&self, l: &[f32], r: &[f32]) {
        record(&self.pre, self.written.load(Ordering::Relaxed), mono(l, r));
    }

    pub fn write_post(&self, l: &[f32], r: &[f32]) {
        let from = self.written.load(Ordering::Relaxed);
        record(&self.post, from, mono(l, r));
        self.written.store(from + l.len().min(r.len()) as u64, Ordering::Release);
    }

    pub fn write(&self, l: &[f32], r: &[f32]) {
        self.write_pre(l, r);
        self.write_post(l, r);
    }

    fn window(&self, pre: &mut [f32], post: &mut [f32]) -> bool {
        if self.serving.load(Ordering::Acquire) != self.aimed.load(Ordering::Relaxed) {
            return false;
        }
        let written = self.written.load(Ordering::Acquire);
        if written < WINDOW as u64 {
            return false;
        }
        let from = (written - WINDOW as u64) as usize;
        for (i, (pre, post)) in pre.iter_mut().zip(post.iter_mut()).enumerate() {
            let at = (from + i) % RING;
            *pre = f32::from_bits(self.pre[at].load(Ordering::Relaxed));
            *post = f32::from_bits(self.post[at].load(Ordering::Relaxed));
        }
        pre.iter().chain(post.iter()).any(|sample| sample.abs() > SILENCE)
    }
}

enum Rebin {
    Peak { lo: usize, hi: usize },
    Lerp { bin: usize, frac: f32 },
}

impl Rebin {
    fn read(&self, bins: &[f32]) -> f32 {
        match *self {
            Rebin::Peak { lo, hi } => bins[lo..=hi].iter().fold(PEAK_SEED_DB, |peak, db| peak.max(*db)),
            Rebin::Lerp { bin, frac } => bins[bin] + (bins[bin + 1] - bins[bin]) * frac,
        }
    }
}

struct Cell {
    rebin: Rebin,
    tilt_db: f32,
}

fn hz_at(position: f64) -> f64 {
    LO_HZ * (HI_HZ / LO_HZ).powf(position / (CELLS - 1) as f64)
}

fn plan(sample_rate: f64) -> Vec<Cell> {
    let bin_hz = sample_rate / WINDOW as f64;
    let top = BINS - 1;
    (0..CELLS)
        .map(|i| {
            let position = i as f64;
            let centre = hz_at(position);
            let lo = ((hz_at(position - 0.5) / bin_hz).ceil() as usize).clamp(1, top);
            let hi = ((hz_at(position + 0.5) / bin_hz).floor() as usize).clamp(1, top);
            let rebin = if lo <= hi {
                Rebin::Peak { lo, hi }
            } else {
                let at = (centre / bin_hz).clamp(1.0, (top - 1) as f64);
                Rebin::Lerp { bin: at.floor() as usize, frac: at.fract() as f32 }
            };
            Cell { rebin, tilt_db: (TILT_DB_PER_OCTAVE * (centre / TILT_HZ).log2()) as f32 }
        })
        .collect()
}

fn bin_db(magnitude: f64) -> f32 {
    (20.0 * ((2.0 * magnitude).max(MAGNITUDE_FLOOR) / BINS as f64).log10()) as f32
}

fn finite(sample: f32) -> f64 {
    if sample.is_finite() { f64::from(sample) } else { 0.0 }
}

pub struct Analyzer {
    fft: Fft,
    window: Vec<f64>,
    plan: Vec<Cell>,
    pre_window: Vec<f32>,
    post_window: Vec<f32>,
    re: Vec<f64>,
    im: Vec<f64>,
    pre_bins: Vec<f32>,
    post_bins: Vec<f32>,
    pre_cells: Vec<f32>,
    post_cells: Vec<f32>,
}

impl Analyzer {
    pub fn new(sample_rate: f64) -> Analyzer {
        let window = (0..WINDOW).map(|i| HANN_SCALE * (1.0 - (std::f64::consts::TAU * i as f64 / WINDOW as f64).cos())).collect();
        Analyzer {
            fft: Fft::new(WINDOW),
            window,
            plan: plan(sample_rate),
            pre_window: vec![0.0; WINDOW],
            post_window: vec![0.0; WINDOW],
            re: vec![0.0; WINDOW],
            im: vec![0.0; WINDOW],
            pre_bins: vec![REST_DB; BINS],
            post_bins: vec![REST_DB; BINS],
            pre_cells: vec![REST_DB; CELLS],
            post_cells: vec![REST_DB; CELLS],
        }
    }

    pub fn analyze(&mut self, tap: &Tap) -> bool {
        if !tap.window(&mut self.pre_window, &mut self.post_window) {
            return false;
        }
        for (((re, im), (pre, post)), w) in self.re.iter_mut().zip(&mut self.im).zip(self.pre_window.iter().zip(&self.post_window)).zip(&self.window) {
            *re = finite(*pre) * w;
            *im = finite(*post) * w;
        }
        self.fft.forward(&mut self.re, &mut self.im);
        for k in 1..BINS {
            let (ar, ai, br, bi) = (self.re[k], self.im[k], self.re[WINDOW - k], self.im[WINDOW - k]);
            self.pre_bins[k] = bin_db((0.5 * (ar + br)).hypot(0.5 * (ai - bi)));
            self.post_bins[k] = bin_db((0.5 * (ai + bi)).hypot(0.5 * (br - ar)));
        }
        for (cell, (pre, post)) in self.plan.iter().zip(self.pre_cells.iter_mut().zip(&mut self.post_cells)) {
            *pre = cell.rebin.read(&self.pre_bins) + cell.tilt_db;
            *post = cell.rebin.read(&self.post_bins) + cell.tilt_db;
        }
        true
    }

    pub fn cells(&self) -> (&[f32], &[f32]) {
        (&self.pre_cells, &self.post_cells)
    }
}

pub struct Spectrum {
    pre: [f32; CELLS],
    post: [f32; CELLS],
}

impl Default for Spectrum {
    fn default() -> Spectrum {
        Spectrum::new()
    }
}

fn settle(lane: &mut [f32], fresh: Option<&[f32]>, attack: f32, release: f32) {
    for (i, value) in lane.iter_mut().enumerate() {
        let target = fresh.map_or(REST_DB, |fresh| fresh[i]);
        let target = if target.is_finite() { target } else { REST_DB };
        let rate = if target > *value { attack } else { release };
        *value += rate * (target - *value);
    }
}

impl Spectrum {
    pub fn new() -> Spectrum {
        Spectrum { pre: [REST_DB; CELLS], post: [REST_DB; CELLS] }
    }

    pub fn rest(&mut self) {
        *self = Spectrum::new();
    }

    pub fn pre(&self) -> &[f32] {
        &self.pre
    }

    pub fn post(&self) -> &[f32] {
        &self.post
    }

    pub fn live(&self) -> bool {
        self.pre.iter().chain(&self.post).any(|db| *db > FLOOR_DB)
    }

    pub fn step(&mut self, fresh: Option<(&[f32], &[f32])>, seconds: f32) -> bool {
        let was = self.live();
        let dt = seconds.clamp(STEP_MIN_SECONDS, STEP_MAX_SECONDS);
        let attack = 1.0 - (-dt / ATTACK_SECONDS).exp();
        let release = 1.0 - (-dt / RELEASE_SECONDS).exp();
        settle(&mut self.pre, fresh.map(|(pre, _)| pre), attack, release);
        settle(&mut self.post, fresh.map(|(_, post)| post), attack, release);
        was || self.live()
    }
}

fn bar(db: f32, height: i32) -> i32 {
    let t = ((db - FLOOR_DB) / (CEILING_DB - FLOOR_DB)).clamp(0.0, 1.0);
    let h = (t * height as f32).floor() as i32;
    h - h % QUANTUM
}

fn cells_per_column(width: i32) -> f64 {
    (CELLS - 1) as f64 / f64::from(width.max(2) - 1)
}

pub fn hz(width: i32, column: i32) -> f64 {
    hz_at(f64::from(column) * cells_per_column(width))
}

fn column_db(lane: &[f32], column: i32, step: f64) -> f32 {
    let at = f64::from(column) * step;
    let lo = (at - step / 2.0).ceil().max(0.0) as usize;
    let hi = ((at + step / 2.0).floor() as usize).min(CELLS - 1);
    if lo <= hi { lane[lo..=hi].iter().copied().fold(REST_DB, f32::max) } else { lane[(at.round() as usize).min(CELLS - 1)] }
}

pub fn draw(list: &mut DrawList, inner: Rect, lane: &[f32], tone: Tone) {
    if inner.w <= 0 || inner.h <= 0 {
        return;
    }
    let step = cells_per_column(inner.w);
    for column in 0..inner.w {
        let h = bar(column_db(lane, column, step), inner.h);
        if h > 0 {
            list.fill(Rect::new(inner.x + column, inner.y + inner.h - h, 1, h), tone);
        }
    }
}

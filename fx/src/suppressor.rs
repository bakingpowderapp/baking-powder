use super::split::{Split, Voicing, spaced};
use crate::params::{self, SUPPRESSOR_BANDS};
use crate::{Clock, Unit};
use sdk::dsp::crossover;
use sdk::dsp::{Mix, Ring, sanitize, settle, smoothing};
use sdk::spectrum::{CURVE, CURVE_PER_OCTAVE, curve_hz};
use std::f64::consts::PI;

const CELLS: usize = CURVE;
const PER_OCTAVE: f64 = CURVE_PER_OCTAVE;
const BANDS: usize = SUPPRESSOR_BANDS;
const TOP_SHARE: f64 = 0.45;
const HOPS_PER_SECOND: f64 = 750.0;
const RING_MIN: usize = 256;
const RING_PAD: usize = 16;
const DC_TICKLE: f64 = 1e-25;
const POWER_FLOOR: f64 = 1e-20;
const LEVEL_FLOOR_SECONDS: f64 = 0.002;
const LEVEL_CYCLES: f64 = 8.0;
const WIDEST_CELLS: f64 = 10.0;
const DETAIL_PER_HALVING: f64 = 3.0;
const GAUSSIAN_FWHM: f64 = 2.354_820_045;
const NARROW_SIGMA: f64 = 0.3;
const KERNEL_REACH: usize = 13;
const GUARD: usize = 3;
const WINDOW: usize = 11;
const STAGE_SHARE: f64 = 0.643_594_252_9;
const LEAK_REACH: usize = 24;
const SHARPEN_PASSES: usize = 2;
const SHARPEN_FLOOR: f64 = 0.001;
const FLUSH: f64 = 1e-18;
const SKIRT_REACH: usize = 8;
const UNMIX_REACH: usize = 12;
const UNMIX_PASSES: usize = 40;
const WEIGHT_DB_PER_DECADE: f64 = 10.0;
const WEIGHT_HZ: f64 = 2000.0;
const WEIGHT_LEAST_DB: f64 = -14.0;
const WEIGHT_MOST_DB: f64 = 2.0;
const FLOOR_RANGE_DB: f64 = 24.0;
const CUT_PER_DEPTH_DB: f64 = 1.45;
const CUT_PER_DETAIL_OCTAVES: f64 = 0.2;
const DETAIL_MIDDLE: f64 = 5.0;
const MIDPOINT_DB: f64 = 24.5;
const MIDPOINT_PER_DEPTH_DB: f64 = 2.0;
const KNEE_DB: f64 = 3.8;
const KNEE_PER_DEPTH_DB: f64 = 0.75;
const KNEE_FROM_DEPTH: f64 = 5.0;
const ATTACK_SECONDS: f64 = 0.005;
const ATTACK_PER_STEP: f64 = 1.55;
const RELEASE_SECONDS: f64 = 0.012;
const RELEASE_PER_STEP: f64 = 1.42;
const IDLE_DB: f64 = 0.05;
const HEARD_SECONDS: f64 = 0.030;

fn law(prominence: f64, flat: f64, depth: f64, detail: f64) -> f64 {
    let most = CUT_PER_DEPTH_DB * depth * 2f64.powf(CUT_PER_DETAIL_OCTAVES * (detail - DETAIL_MIDDLE));
    let midpoint = MIDPOINT_DB - MIDPOINT_PER_DEPTH_DB * depth;
    let knee = KNEE_DB + KNEE_PER_DEPTH_DB * (depth - KNEE_FROM_DEPTH).max(0.0);
    let logistic = |p: f64| 1.0 / (1.0 + ((midpoint - p) / knee).exp());
    let floor = logistic(flat);
    most * ((logistic(prominence) - floor) / (1.0 - floor)).max(0.0)
}

fn floor_db(levels: &[f64; CELLS], i: usize) -> f64 {
    let below = i.saturating_sub(WINDOW)..(i + 1).saturating_sub(GUARD);
    let above = (i + GUARD).min(CELLS)..(i + WINDOW + 1).min(CELLS);
    let window = levels[below.clone()].iter().chain(&levels[above.clone()]);
    let loudest = window.clone().fold(f64::NEG_INFINITY, |most, level| most.max(*level));
    let count = (below.len() + above.len()).max(1) as f64;
    window.map(|level| level.max(loudest - FLOOR_RANGE_DB)).sum::<f64>() / count
}

fn decibels(power: f64) -> f64 {
    10.0 * (power + POWER_FLOOR).log10()
}

#[derive(Clone, Copy, Default)]
struct Bell {
    a1: f64,
    a2: f64,
    a3: f64,
    m1: f64,
}

impl Bell {
    fn of(g: f64, k: f64, cut_db: f64) -> Bell {
        let a = 10f64.powf(-cut_db / 40.0);
        let k = k / a;
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        Bell { a1, a2, a3: g * a2, m1: k * (a * a - 1.0) }
    }

    fn toward(&self, aim: &Bell, hop: usize) -> Bell {
        let share = 1.0 / hop as f64;
        Bell { a1: share * (aim.a1 - self.a1), a2: share * (aim.a2 - self.a2), a3: share * (aim.a3 - self.a3), m1: share * (aim.m1 - self.m1) }
    }

    fn advance(&mut self, by: &Bell) {
        self.a1 += by.a1;
        self.a2 += by.a2;
        self.a3 += by.a3;
        self.m1 += by.m1;
    }
}

#[derive(Clone, Copy, Default)]
struct Svf {
    s1: f64,
    s2: f64,
}

impl Svf {
    fn tick(&mut self, x: f64, bell: &Bell) -> f64 {
        let v3 = x - self.s2;
        let v1 = bell.a1 * self.s1 + bell.a2 * v3;
        let v2 = self.s2 + bell.a2 * self.s1 + bell.a3 * v3;
        self.s1 = 2.0 * v1 - self.s1;
        self.s2 = 2.0 * v2 - self.s2;
        v1
    }
}

struct Ears {
    k: f64,
    a1: [f64; CELLS],
    a2: [f64; CELLS],
    a3: [f64; CELLS],
    first: [Svf; CELLS],
    second: [Svf; CELLS],
    coeff: [f64; CELLS],
    leak: [f64; LEAK_REACH + 1],
    power: [f64; CELLS],
}

impl Ears {
    fn new(g: &[f64; CELLS], k: f64, coeff: [f64; CELLS]) -> Ears {
        let k = k / STAGE_SHARE;
        let bells = g.map(|g| Bell::of(g, k, 0.0));
        let leak = std::array::from_fn(|d| {
            let ratio = 2f64.powf(d as f64 / PER_OCTAVE);
            let x = (ratio - 1.0 / ratio) / k;
            (1.0 / (1.0 + x * x)).powi(2)
        });
        Ears { k, a1: bells.map(|bell| bell.a1), a2: bells.map(|bell| bell.a2), a3: bells.map(|bell| bell.a3), first: [Svf::default(); CELLS], second: [Svf::default(); CELLS], coeff, leak, power: [0.0; CELLS] }
    }

    fn sharpened(&self) -> [f64; CELLS] {
        let mut own = self.power;
        for _ in 0..SHARPEN_PASSES {
            for i in 0..CELLS {
                let mut spilt = 0.0;
                for (d, leak) in self.leak.iter().enumerate().skip(1) {
                    if i >= d {
                        spilt += leak * own[i - d];
                    }
                    if i + d < CELLS {
                        spilt += leak * own[i + d];
                    }
                }
                own[i] = (self.power[i] - spilt).max(SHARPEN_FLOOR * self.power[i]);
            }
        }
        own
    }

    fn hear(&mut self, x: f64) {
        for b in 0..CELLS {
            let stage = Bell { a1: self.a1[b], a2: self.a2[b], a3: self.a3[b], m1: 0.0 };
            let once = self.k * self.first[b].tick(x, &stage);
            let band = self.k * self.second[b].tick(once, &stage);
            self.power[b] += self.coeff[b] * (band * band - self.power[b]);
        }
    }

    fn clear(&mut self) {
        self.first = [Svf::default(); CELLS];
        self.second = [Svf::default(); CELLS];
        self.power = [0.0; CELLS];
    }
}

#[derive(Clone, Copy)]
struct Kernel {
    detail: f64,
    taps: [f64; 2 * KERNEL_REACH + 1],
    reach: usize,
}

impl Kernel {
    fn of(detail: f64) -> Kernel {
        let width = WIDEST_CELLS * 2f64.powf(-detail / DETAIL_PER_HALVING);
        let sigma = width / GAUSSIAN_FWHM;
        let reach = if sigma < NARROW_SIGMA { 0 } else { ((3.0 * sigma).ceil() as usize).min(KERNEL_REACH) };
        let mut taps = [0.0; 2 * KERNEL_REACH + 1];
        for (offset, tap) in taps.iter_mut().enumerate() {
            let distance = offset as f64 - KERNEL_REACH as f64;
            *tap = if distance.abs() as usize <= reach && (reach > 0 || distance == 0.0) { (-0.5 * (distance / sigma.max(NARROW_SIGMA)).powi(2)).exp() } else { 0.0 };
        }
        Kernel { detail, taps, reach }
    }

    fn widen_at(&self, cut: &[f64; CELLS], i: usize) -> f64 {
        let (lo, hi) = (i.saturating_sub(self.reach), (i + self.reach).min(CELLS - 1));
        self.taps[lo + KERNEL_REACH - i..=hi + KERNEL_REACH - i].iter().zip(&cut[lo..=hi]).fold(0.0, |most: f64, (tap, cut)| most.max(tap * cut))
    }
}

#[derive(Clone, Copy)]
struct Pace {
    attack: f64,
    release: f64,
}

fn unmix(k: f64) -> [f64; 2 * UNMIX_REACH + 1] {
    let skirt: [f64; SKIRT_REACH + 1] = std::array::from_fn(|d| {
        let ratio = 2f64.powf(d as f64 / PER_OCTAVE);
        let x = (ratio - 1.0 / ratio) / k;
        1.0 / (1.0 + x * x)
    });
    let mut x = [0.0; 2 * UNMIX_REACH + 1];
    for _ in 0..UNMIX_PASSES {
        let mut next = [0.0; 2 * UNMIX_REACH + 1];
        for (i, out) in next.iter_mut().enumerate() {
            let mut acc = if i == UNMIX_REACH { 1.0 } else { 0.0 };
            for (d, s) in skirt.iter().enumerate().skip(1) {
                if i >= d {
                    acc -= s * x[i - d];
                }
                if i + d < x.len() {
                    acc -= s * x[i + d];
                }
            }
            *out = acc;
        }
        x = next;
    }
    x
}

pub struct Suppressor {
    sample_rate: f64,
    hop_frames: usize,
    room: usize,
    ring_l: Ring,
    ring_r: Ring,
    mix: Mix,
    tickle: f64,
    k: f64,
    g: [f64; CELLS],
    plain: [Bell; CELLS],
    weight: [f64; CELLS],
    ears: Ears,
    kernels: [Kernel; BANDS],
    owner: [usize; CELLS],
    unmix: [f64; 2 * UNMIX_REACH + 1],
    env: [f64; CELLS],
    shown: [f32; CELLS],
    now: [Bell; CELLS],
    aim: [Bell; CELLS],
    delta: [Bell; CELLS],
    active: [bool; CELLS],
    left: [Svf; CELLS],
    right: [Svf; CELLS],
    hop: usize,
    gauge: f32,
    heard: [f32; BANDS],
    heard_smooth: f32,
    fresh: bool,
    split: Split,
    wet_l: Vec<f32>,
    wet_r: Vec<f32>,
}

impl Suppressor {
    fn update(&mut self, params: &params::Suppressor) {
        let count = params.count();
        for (kernel, band) in self.kernels.iter_mut().zip(&params.bands).take(count) {
            if kernel.detail != band.detail {
                *kernel = Kernel::of(band.detail);
            }
        }
        let crossovers = spaced(params.crossovers_hz, params::Suppressor::CROSSOVER_RATIO, count, self.sample_rate);
        for (cell, owner) in self.owner.iter_mut().enumerate() {
            let hz = curve_hz(cell);
            *owner = crossovers[..count - 1].iter().filter(|split| hz >= **split).count();
        }
        let sr = self.sample_rate;
        let per_hop = |seconds: f64| 1.0 - (-(self.hop_frames as f64) / (seconds * sr)).exp();
        let paces: [Pace; BANDS] = std::array::from_fn(|b| Pace { attack: per_hop(ATTACK_SECONDS * ATTACK_PER_STEP.powf(params.bands[b].attack)), release: per_hop(RELEASE_SECONDS * RELEASE_PER_STEP.powf(params.bands[b].release)) });
        let levels = self.ears.sharpened().map(decibels);
        let mut wanted = [0.0; CELLS];
        for (i, (level, cut)) in levels.iter().zip(wanted.iter_mut()).enumerate() {
            let band = &params.bands[self.owner[i]];
            if band.on {
                let prominence = level - floor_db(&levels, i) + self.weight[i];
                *cut = law(prominence, self.weight[i], band.depth, band.detail);
            }
        }
        let mut most = 0.0f64;
        for (i, env) in self.env.iter_mut().enumerate() {
            let owner = self.owner[i];
            let target = if params.bands[owner].on { self.kernels[owner].widen_at(&wanted, i) } else { 0.0 };
            let pace = paces[owner];
            *env += if target > *env { pace.attack } else { pace.release } * (target - *env);
            if target < IDLE_DB && *env < IDLE_DB {
                *env = 0.0;
            }
            most = most.max(*env);
        }
        self.gauge = most as f32;
        self.shown = self.env.map(|env| env as f32);
        for i in 0..CELLS {
            let mut cut = 0.0;
            for j in i.saturating_sub(UNMIX_REACH)..=(i + UNMIX_REACH).min(CELLS - 1) {
                cut += self.unmix[j + UNMIX_REACH - i] * self.env[j];
            }
            self.now[i] = self.aim[i];
            self.aim[i] = if cut < IDLE_DB { self.plain[i] } else { Bell::of(self.g[i], self.k, cut) };
            self.delta[i] = self.now[i].toward(&self.aim[i], self.hop_frames);
            self.active[i] = self.now[i].m1 != 0.0 || self.aim[i].m1 != 0.0;
            for side in [&mut self.left[i], &mut self.right[i]] {
                if !self.active[i] || side.s1.abs() + side.s2.abs() < FLUSH {
                    *side = Svf::default();
                }
            }
        }
    }

    fn block(&mut self, l: &mut [f32], r: &mut [f32], params: &params::Suppressor, wet: f32) {
        let n = l.len();
        let count = params.count();
        let lookahead = params.lookahead(self.sample_rate);
        for (i, (dry_l, dry_r)) in l.iter().zip(r.iter()).enumerate() {
            self.ring_l.push(*dry_l);
            self.ring_r.push(*dry_r);
            self.tickle = -self.tickle;
            self.ears.hear(0.5 * (f64::from(*dry_l) + f64::from(*dry_r)) + self.tickle);
            if self.hop == 0 {
                self.update(params);
            }
            self.hop = (self.hop + 1) % self.hop_frames;
            let (dry_l, dry_r) = (self.ring_l.tap(lookahead + 1), self.ring_r.tap(lookahead + 1));
            let (mut wet_l, mut wet_r) = (f64::from(dry_l), f64::from(dry_r));
            for b in 0..CELLS {
                if !self.active[b] {
                    continue;
                }
                self.now[b].advance(&self.delta[b]);
                let bell = self.now[b];
                wet_l += bell.m1 * self.left[b].tick(wet_l, &bell);
                wet_r += bell.m1 * self.right[b].tick(wet_r, &bell);
            }
            let mix = self.mix.tick(wet);
            self.wet_l[i] = (1.0 - mix) * dry_l + mix * wet_l as f32;
            self.wet_r[i] = (1.0 - mix) * dry_r + mix * wet_r as f32;
        }
        let soloing = params.bands[..count].iter().any(|band| band.on && band.solo);
        let targets: [f32; BANDS] = std::array::from_fn(|b| {
            let band = &params.bands[b];
            if if soloing { band.on && band.solo } else { !(band.on && band.mute) } { 1.0 } else { 0.0 }
        });
        if self.fresh {
            self.heard = targets;
            self.fresh = false;
        }
        let smooth = self.heard_smooth;
        self.split.run(&self.wet_l[..n], &self.wet_r[..n], Voicing::new(params.crossovers_hz, params.slope, count, params::Suppressor::CROSSOVER_RATIO));
        let bands = self.split.bands();
        for (i, (out_l, out_r)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
            let mut sum = [0.0f32; 2];
            for (b, heard) in self.heard.iter_mut().enumerate() {
                *heard = settle(*heard, targets[b], smooth);
                if b < count {
                    sum[0] += bands[b][0][i] * *heard;
                    sum[1] += bands[b][1][i] * *heard;
                }
            }
            *out_l = sum[0];
            *out_r = sum[1];
        }
    }
}

impl Unit for Suppressor {
    type Params = params::Suppressor;

    fn new(sample_rate: f64, max_frames: usize) -> Suppressor {
        let k = 2f64.powf(0.5 / PER_OCTAVE) - 2f64.powf(-0.5 / PER_OCTAVE);
        let top = TOP_SHARE * sample_rate;
        let hz: [f64; CELLS] = std::array::from_fn(|b| curve_hz(b).min(top));
        let g = hz.map(|hz| (PI * hz / sample_rate).tan());
        let plain = g.map(|g| Bell::of(g, k, 0.0));
        let ring = (params::Suppressor::DEFAULT.lookahead(sample_rate) + crossover::half(sample_rate) + RING_PAD).max(RING_MIN);
        let room = max_frames.max(1);
        Suppressor {
            sample_rate,
            hop_frames: (sample_rate / HOPS_PER_SECOND).round().max(1.0) as usize,
            room,
            ring_l: Ring::new(ring),
            ring_r: Ring::new(ring),
            mix: Mix::new(sample_rate),
            tickle: DC_TICKLE,
            k,
            g,
            plain,
            weight: hz.map(|hz| (WEIGHT_DB_PER_DECADE * (hz / WEIGHT_HZ).log10()).clamp(WEIGHT_LEAST_DB, WEIGHT_MOST_DB)),
            ears: Ears::new(&g, k, hz.map(|hz| smoothing(LEVEL_FLOOR_SECONDS.max(LEVEL_CYCLES / hz), sample_rate))),
            kernels: [Kernel::of(params::SuppressorBand::DEFAULT.detail); BANDS],
            owner: [0; CELLS],
            unmix: unmix(k),
            env: [0.0; CELLS],
            shown: [0.0; CELLS],
            now: plain,
            aim: plain,
            delta: [Bell::default(); CELLS],
            active: [false; CELLS],
            left: [Svf::default(); CELLS],
            right: [Svf::default(); CELLS],
            hop: 0,
            gauge: 0.0,
            heard: [1.0; BANDS],
            heard_smooth: smoothing(HEARD_SECONDS, sample_rate) as f32,
            fresh: true,
            split: Split::new(sample_rate, room),
            wet_l: vec![0.0; room],
            wet_r: vec![0.0; room],
        }
    }

    fn restart(&mut self) {
        self.ring_l.clear();
        self.ring_r.clear();
        self.ears.clear();
        self.env = [0.0; CELLS];
        self.shown = [0.0; CELLS];
        self.now = self.plain;
        self.aim = self.plain;
        self.delta = [Bell::default(); CELLS];
        self.active = [false; CELLS];
        self.left = [Svf::default(); CELLS];
        self.right = [Svf::default(); CELLS];
        self.hop = 0;
        self.gauge = 0.0;
        self.split.restart();
    }

    fn gauge(&self) -> f32 {
        self.gauge
    }

    fn curve(&self) -> Option<&[f32; CELLS]> {
        Some(&self.shown)
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], params: &params::Suppressor, wet: f32, _: &Clock) {
        let n = l.len().min(r.len());
        let (l, r) = (&mut l[..n], &mut r[..n]);
        sanitize(l, r);
        for start in (0..n.max(1)).step_by(self.room) {
            let end = (start + self.room).min(n);
            if start < end {
                self.block(&mut l[start..end], &mut r[start..end], params, wet);
            }
        }
    }
}

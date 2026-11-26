use sdk::dsp::filter::{Coeffs, Path, State, Tuned};
use sdk::dsp::noise::{white, white_of, xorshift32, xorshift32_after};
use sdk::dsp::svf::{Biquad, BiquadCoeffs, high_pass, low_pass};
use sdk::dsp::{self, DcBlock, Halfband, PitchShifter, Ring, lp_coeff, one_pole_lp, widen, wrap};
use crate::lfo_shape::{begun, turns};
use crate::params::{Age, EarlyPattern, Tank};
use crate::silence::Silence;
use crate::{Beat, Clock, Unit, params};
use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_4, PI};

const FS0: f64 = 29_761.0;
const PLATE_DECAY_STAGES: f64 = 4.0;
const DIFFUSER: [(usize, f32); 4] = [(142, 0.75), (107, 0.75), (379, 0.625), (277, 0.625)];
const G_MOD: f32 = -0.70;
const MOD_REACH: f32 = 0.9;
const ATTACK_SHARES: [f64; 6] = [0.03, 0.07, 0.12, 0.18, 0.25, 0.35];
const ATTACK_GAIN: f32 = 0.5;
const ATTACK_SNAP: f32 = 1e-3;
const WIDTH_SNAP: f32 = 1e-4;
const WANDER_HEADROOM: usize = 16;
const EARLY_TAPS: usize = 8;
const EARLY_TAPS_L: [(f32, f32); EARLY_TAPS] = [(0.000, 0.542), (0.071, -0.445), (0.163, 0.401), (0.247, -0.331), (0.358, 0.298), (0.491, -0.255), (0.667, 0.222), (0.853, -0.184)];
const EARLY_TAPS_R: [(f32, f32); EARLY_TAPS] = [(0.029, -0.536), (0.118, 0.452), (0.204, -0.395), (0.301, 0.339), (0.424, -0.294), (0.572, 0.260), (0.738, -0.220), (0.936, 0.186)];
const ROOM_L: [(f32, f32); EARLY_TAPS] = [(0.000, 0.600), (0.090, 0.450), (0.160, -0.400), (0.290, 0.330), (0.410, -0.250), (0.580, 0.180), (0.740, -0.120), (0.900, 0.080)];
const ROOM_R: [(f32, f32); EARLY_TAPS] = [(0.020, 0.580), (0.110, 0.440), (0.180, -0.400), (0.310, 0.330), (0.430, -0.250), (0.600, 0.180), (0.760, -0.120), (0.920, 0.080)];
const CHAMBER_L: [(f32, f32); EARLY_TAPS] = [(0.000, 0.354), (0.130, -0.354), (0.260, 0.354), (0.390, -0.354), (0.520, 0.354), (0.650, -0.354), (0.780, 0.354), (0.910, -0.354)];
const CHAMBER_R: [(f32, f32); EARLY_TAPS] = [(0.065, -0.354), (0.195, 0.354), (0.325, -0.354), (0.455, 0.354), (0.585, -0.354), (0.715, 0.354), (0.845, -0.354), (0.975, 0.354)];
const STAGE_L: [(f32, f32); EARLY_TAPS] = [(0.000, 0.700), (0.220, 0.450), (0.470, -0.350), (0.710, 0.250), (0.950, -0.150), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0)];
const STAGE_R: [(f32, f32); EARLY_TAPS] = [(0.110, -0.650), (0.330, 0.480), (0.580, 0.350), (0.830, -0.250), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0)];
const HALL_L: [(f32, f32); EARLY_TAPS] = [(0.420, 0.620), (0.570, 0.450), (0.750, -0.380), (0.880, 0.300), (0.970, -0.220), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0)];
const HALL_R: [(f32, f32); EARLY_TAPS] = [(0.440, 0.600), (0.610, -0.450), (0.780, 0.380), (0.920, -0.300), (0.990, 0.220), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0)];
const ATTACK_SHAPE_REACH: f32 = 0.3;
const DISPERSE_HZ: f64 = 1200.0;
const DISPERSE_TOP_HZ: f64 = 20_000.0;
const DISPERSE_Q: f64 = 2.0;
const DISPERSE_STAGES: usize = 8;
const DISPERSE_DELAY_MS: f64 = 2.8;
const AGE_BANDWIDTH_HZ: [f64; 2] = [11_000.0, 6_500.0];
const AGE_STEPS: f32 = 2048.0;
const AGE_HOLD_HZ: f64 = 24_000.0;
const ARM_A: Geometry = Geometry { modulated: 672.0, delay1: 4453.0, decay: 1800.0, delay2: 3720.0, same: [353.0, 3627.0, 1228.0, 2673.0], cross: [1990.0, 187.0, 1066.0], wander: [(0.23, 0xA1A1_0001), (0.37, 0xA2A2_0002)], jitter_seed: 0x5EED_0001, shimmer_seed: 0x1234_5677 };
const ARM_B: Geometry = Geometry { modulated: 908.0, delay1: 4217.0, decay: 2656.0, delay2: 3163.0, same: [266.0, 2974.0, 1913.0, 1996.0], cross: [2111.0, 335.0, 121.0], wander: [(0.29, 0xB1B1_0003), (0.41, 0xB2B2_0004)], jitter_seed: 0x5EED_0002, shimmer_seed: 0x89AB_CDEF };
const WANDER_GLIDE_S: f64 = 1.5;
const JITTER_GLIDE_TURNS: f64 = 0.5;
const FORGOTTEN: f64 = -133.0;
const RECALL_LEAST: f64 = 8.0;
const RECALL_REACH: i64 = 64;
const RECALL_LANES: usize = 32;
const RECALL_FARTHEST: i64 = 1 << 30;
const NEVER: u32 = u32::MAX;
const LINES: usize = 16;
const LINE_MS: [f64; LINES] = [11.3, 14.9, 18.7, 22.1, 25.9, 29.3, 33.7, 38.3, 43.1, 47.9, 53.3, 58.1, 63.7, 69.1, 75.7, 83.3];
const LINE_DIFFUSER_MS: [f64; LINES] = [1.1, 1.7, 2.3, 2.9, 3.7, 4.3, 5.1, 5.9, 1.3, 1.9, 2.7, 3.1, 3.9, 4.7, 5.3, 6.1];
const LINE_SIGNS: [f32; LINES] = [1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0];
const LINE_SIDES: [f32; LINES] = [1.0, -1.0, -1.0, 1.0, 1.0, -1.0, -1.0, 1.0, 1.0, -1.0, -1.0, 1.0, 1.0, -1.0, -1.0, 1.0];
const LINE_FLIPS: [f32; LINES] = [1.0, 1.0, -1.0, 1.0, -1.0, -1.0, 1.0, -1.0, -1.0, 1.0, 1.0, -1.0, 1.0, -1.0, -1.0, 1.0];
const LINE_TURN: usize = 5;
const DAMP_TIMES: f64 = 0.5;
const LATTICE_STRETCH: f64 = 1.026;
const LATTICE_STRETCH_PER_DIFFUSION: f64 = 0.15;
const LINE_WANDER: [(f64, u32); 2] = [(0.31, 0xC1C1_0005), (0.43, 0xC2C2_0006)];
const LINE_JITTER_SEED: u32 = 0x7A77_0007;
const LINE_SWAYS: usize = LINES / 2;
const LATTICE_OUT: f32 = 0.34;
const LATTICE_SCALE: f32 = 0.25;
const SHIMMER_GRAIN_MS: f64 = 80.0;
const SHIMMER_JITTER_MS: f64 = 3.0;
const SHIMMER_Q: f64 = 0.707_106_77;
const SHIMMER_LOW_HZ: f64 = 200.0;
const SHIMMER_HIGH_HZ: f64 = 9000.0;
const SHIMMER_WANDER_MS: f64 = 0.0625;
const DC_CUTOFF_HZ: f64 = 35.0;
const DECAY_RISE_HZ: f64 = 150.0;
const SMOOTH_S: f64 = 0.030;
const TICKLE: f32 = 1e-20;
const FREEZE_SNAP: f64 = 0.9999;
const FREEZE_EPS: f32 = 1e-6;
const DUCK_DETECT_ATTACK_S: f64 = 0.001;
const DUCK_DETECT_RELEASE_S: f64 = 0.050;
const TAPS: usize = params::Reverb::TAPS;
const FILTERS: usize = params::Reverb::FILTERS;
const SCATTER_TAPS: usize = 24;
const SCATTER_MS: f64 = 16.0;
const SCATTER_SEEDS: [u32; 2] = [0x5CA7_7E01, 0x5CA7_7E02];
const ENTRY_DOORS: usize = 4;
const ENTRY_MS: f64 = 40.0;
const ENTRY_SEED: u32 = 0xD00B_0001;
const SPLIT_Q: f64 = FRAC_1_SQRT_2;
const PLATEAU_TAPS: usize = params::Reverb::PLATEAU_COPIES;
const PLATEAU_SEED: u32 = 0x9E37_79B9;
const PLATEAU_FADE_MS: f64 = 2.0;
const PLATEAU_LN_1000: f32 = 6.907_755;
const PLATEAU_MAX_CARPET: f32 = 16.0;
const PLATEAU_SETTLE: f32 = 1e-6;
const DRIFT_SEED: u32 = 0xD21F_7001;
const DRIFT_HZ: (f64, f64) = (2.0, 6.0);
const QUADRATURE_I: [f32; 4] = [0.402_192_12, 0.856_171_1, 0.972_290_95, 0.995_288_5];
const QUADRATURE_Q: [f32; 4] = [0.692_387_8, 0.936_065_4, 0.988_229_5, 0.998_748_8];
const CROSS_HEADROOM: usize = 8;
const HOP: usize = 64;

#[derive(Clone, Copy)]
struct Geometry {
    modulated: f32,
    delay1: f32,
    decay: f32,
    delay2: f32,
    same: [f32; 4],
    cross: [f32; 3],
    wander: [(f64, u32); 2],
    jitter_seed: u32,
    shimmer_seed: u32,
}

impl Geometry {
    fn scaled(&self, scale: f64) -> Geometry {
        let r = |base: f32| (f64::from(base) * scale).round() as f32;
        Geometry { modulated: r(self.modulated), delay1: r(self.delay1), decay: r(self.decay), delay2: r(self.delay2), same: self.same.map(r), cross: self.cross.map(r), ..*self }
    }

    fn total(&self) -> f32 {
        self.modulated + self.delay1 + self.decay + self.delay2
    }
}

fn shelved(x: f32, low: f32, shelf: f32) -> f32 {
    if shelf == 1.0 { x } else { x + (shelf - 1.0) * low }
}

fn shelved_high(x: f32, low: f32, shelf: f32) -> f32 {
    if shelf == 1.0 { x } else { x + (shelf - 1.0) * (x - low) }
}

#[derive(Clone, Copy, PartialEq)]
enum Read {
    Straight,
    Cubic,
    Full,
}

impl Read {
    fn of(cubic: bool, full: bool) -> Read {
        if full {
            Read::Full
        } else if cubic {
            Read::Cubic
        } else {
            Read::Straight
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Reader {
    y: f32,
}

impl Reader {
    fn passed(&mut self, ring: &Ring, delay: f64) -> f32 {
        let whole = delay.floor();
        let frac = delay - whole;
        let n = whole as usize;
        let y = if frac == 0.0 {
            ring.tap(n)
        } else if frac >= 0.5 {
            let eta = ((1.0 - frac) / (1.0 + frac)) as f32;
            eta * (ring.tap(n) - self.y) + ring.tap(n + 1)
        } else if n >= 2 {
            let reach = 1.0 + frac;
            let eta = ((1.0 - reach) / (1.0 + reach)) as f32;
            eta * (ring.tap(n - 1) - self.y) + ring.tap(n)
        } else {
            ring.linear(delay)
        };
        self.y = y;
        y
    }

    fn read(&mut self, ring: &Ring, delay: f64, how: Read) -> f32 {
        match how {
            Read::Straight => ring.linear(delay),
            Read::Cubic => ring.hermite(delay),
            Read::Full => self.passed(ring, delay),
        }
    }

    fn allpass(&mut self, ring: &mut Ring, delay: f64, g: f32, x: f32, loss: f32, how: Read) -> f32 {
        let r = loss * self.read(ring, delay, how);
        let v = x - g * r;
        ring.push(v);
        g * v + r
    }
}

fn lossy_allpass(ring: &mut Ring, delay: usize, g: f32, x: f32, loss: f32) -> f32 {
    let r = loss * ring.tap(delay);
    let v = x - g * r;
    ring.push(v);
    g * v + r
}

struct Delayed {
    rings: [Ring; 2],
    readers: [Reader; 2],
    fed: bool,
}

impl Delayed {
    fn new(sample_rate: f64) -> Delayed {
        let span = (params::Reverb::MAX_CROSS_MS * 0.001 * sample_rate).ceil() as usize + CROSS_HEADROOM;
        Delayed { rings: [Ring::new(span), Ring::new(span)], readers: [Reader::default(); 2], fed: false }
    }

    fn clear(&mut self) {
        for ring in &mut self.rings {
            ring.clear();
        }
        self.readers = [Reader::default(); 2];
        self.fed = false;
    }

    fn rest(&mut self) {
        if self.fed {
            self.clear();
        }
    }

    fn tick(&mut self, (a, b): (f32, f32), delay: f64, how: Read) -> (f32, f32) {
        self.fed = true;
        self.rings[0].push(a);
        self.rings[1].push(b);
        let at = delay + 1.0;
        (self.readers[0].read(&self.rings[0], at, how), self.readers[1].read(&self.rings[1], at, how))
    }

    fn crossed(&mut self, (l, r): (f32, f32), delay: f64, level: f32, how: Read) -> (f32, f32) {
        let (far_l, far_r) = self.tick((l, r), delay, how);
        (l + level * far_r, r + level * far_l)
    }
}

fn ratio(low: f32, decay: f32) -> f32 {
    if decay > 0.0 { low / decay } else { 1.0 }
}

fn decay_gain(rt60: f64, sample_rate: f64, loop_samples: f64) -> f32 {
    10f64.powf(-3.0 * loop_samples / (PLATE_DECAY_STAGES * rt60.max(params::Reverb::MIN_DECAY_S) * sample_rate)).clamp(0.0, 0.999) as f32
}

fn line_gain(rt60: f64, sample_rate: f64, samples: f64) -> f32 {
    10f64.powf(-3.0 * samples / (rt60.max(params::Reverb::MIN_DECAY_S) * sample_rate)).clamp(0.0, 0.999) as f32
}

fn fade_gain(db_per_s: f64, seconds: f64) -> f32 {
    10f64.powf(-db_per_s * seconds / 20.0) as f32
}

type Kept<const N: usize, T> = Option<([u64; N], T)>;

fn kept<const N: usize, T: Copy>(slot: &mut Kept<N, T>, inputs: [f64; N], make: impl FnOnce() -> T) -> T {
    let bits = inputs.map(f64::to_bits);
    match *slot {
        Some((kept, value)) if kept == bits => value,
        _ => {
            let value = make();
            *slot = Some((bits, value));
            value
        }
    }
}

fn same(a: &BiquadCoeffs, b: &BiquadCoeffs) -> bool {
    [a.b0, a.b1, a.b2, a.a1, a.a2] == [b.b0, b.b1, b.b2, b.a1, b.a2]
}

fn early(pre: &Ring, base: f64, span: f64, taps: &[(f32, f32); EARLY_TAPS], readers: &mut [Reader; EARLY_TAPS], how: Read) -> f32 {
    taps.iter().zip(readers.iter_mut()).map(|((at, gain), reader)| reader.read(pre, base + f64::from(*at) * span, how) * gain).sum()
}

fn pattern(choice: EarlyPattern) -> [&'static [(f32, f32); EARLY_TAPS]; 2] {
    match choice {
        EarlyPattern::Scatter => [&EARLY_TAPS_L, &EARLY_TAPS_R],
        EarlyPattern::Room => [&ROOM_L, &ROOM_R],
        EarlyPattern::Chamber => [&CHAMBER_L, &CHAMBER_R],
        EarlyPattern::Stage => [&STAGE_L, &STAGE_R],
        EarlyPattern::Hall => [&HALL_L, &HALL_R],
    }
}

fn all_pass(hz: f64, q: f64, sample_rate: f64) -> BiquadCoeffs {
    let omega = 2.0 * PI * hz / sample_rate;
    let alpha = omega.sin() / (2.0 * q);
    let norm = 1.0 + alpha;
    let a1 = -2.0 * omega.cos() / norm;
    let a2 = (1.0 - alpha) / norm;
    BiquadCoeffs { b0: a2, b1: a1, b2: 1.0, a1, a2 }
}

fn stepped(x: f32, steps: f32) -> f32 {
    (x * steps).round() / steps
}

#[derive(Clone, Copy)]
struct Dispersion {
    on: bool,
    stages: usize,
    coeffs: BiquadCoeffs,
    partial: BiquadCoeffs,
}

impl Dispersion {
    fn at(amount: f32, on: bool, coeffs: BiquadCoeffs, sample_rate: f64) -> Dispersion {
        let reach = amount.clamp(0.0, 1.0) * DISPERSE_STAGES as f32;
        let stages = (reach.floor() as usize).min(DISPERSE_STAGES);
        let frac = f64::from(reach - stages as f32);
        let partial_hz = DISPERSE_HZ * (DISPERSE_TOP_HZ / DISPERSE_HZ).powf(1.0 - frac);
        Dispersion { on, stages, coeffs, partial: all_pass(partial_hz.min(sample_rate * 0.45), DISPERSE_Q, sample_rate) }
    }
}

#[derive(Default)]
struct Disperser {
    stages: [Biquad; DISPERSE_STAGES],
}

impl Disperser {
    fn reset(&mut self) {
        for stage in &mut self.stages {
            stage.reset();
        }
    }

    fn settle(&mut self, d: &Dispersion) {
        let live = if d.on { (d.stages + 1).min(DISPERSE_STAGES) } else { 0 };
        for stage in &mut self.stages[live..] {
            stage.reset();
        }
    }

    fn tick(&mut self, x: f32, d: &Dispersion) -> f32 {
        if !d.on {
            return x;
        }
        let mut y = f64::from(x);
        for stage in &mut self.stages[..d.stages] {
            y = stage.run(d.coeffs, y);
        }
        if d.stages < DISPERSE_STAGES {
            y = self.stages[d.stages].run(d.partial, y);
        }
        y as f32
    }
}

fn quantized(x: f32, amount: f32) -> f32 {
    if amount > 0.0 { x + amount * (x.round() - x) } else { x }
}

fn held(gain: f32, hold: f32) -> f32 {
    gain + hold * (1.0 - gain)
}

fn eased(x: f32, target: f32, a: f32) -> f32 {
    let next = x + a * (target - x);
    if next == x && target == 0.0 { 0.0 } else { next }
}

fn rotation(mixing: f32) -> (f32, f32) {
    let (s, c) = (f64::from(mixing) * FRAC_PI_4).sin_cos();
    (s as f32, c as f32)
}

fn butterfly(v: &mut [f32; LINES], (s, c): (f32, f32)) {
    let mut half = 1;
    while half < LINES {
        for start in (0..LINES).step_by(2 * half) {
            for i in start..start + half {
                let (x, y) = (v[i], v[i + half]);
                v[i] = c * x + s * y;
                v[i + half] = s * x - c * y;
            }
        }
        half *= 2;
    }
}

fn hadamard(v: &mut [f32; LINES]) {
    let mut half = 1;
    while half < LINES {
        for start in (0..LINES).step_by(2 * half) {
            for i in start..start + half {
                let (x, y) = (v[i], v[i + half]);
                v[i] = x + y;
                v[i + half] = x - y;
            }
        }
        half *= 2;
    }
    for x in v.iter_mut() {
        *x *= LATTICE_SCALE;
    }
}

fn draws(sparse: f32) -> u64 {
    if sparse <= 0.0 { 1 } else { 2 }
}

fn takes(decider: u32, sparse: f32) -> bool {
    0.5 + 0.5 * white_of(decider) as f32 >= sparse
}

fn drawn(rng: &mut u32, sparse: f32) -> Option<f32> {
    let drawn = white(rng) as f32;
    (sparse <= 0.0 || takes(xorshift32(rng), sparse)).then_some(drawn)
}

fn least_taken(sparse: f32) -> Option<u32> {
    let (mut refused, mut taken) = (0, u32::MAX);
    if takes(refused, sparse) {
        return Some(refused);
    }
    if !takes(taken, sparse) {
        return None;
    }
    while taken - refused > 1 {
        let middle = refused + (taken - refused) / 2;
        if takes(middle, sparse) {
            taken = middle;
        } else {
            refused = middle;
        }
    }
    Some(taken)
}

fn last_taken(seed: u32, least: u32, start: i64, end: i64) -> Option<i64> {
    let strip = (end - start) / RECALL_LANES as i64;
    let first = |lane: usize| start + lane as i64 * strip;
    let mut rng = xorshift32_after(seed, 2 * start as u64);
    let mut rngs = [0; RECALL_LANES];
    for lane in &mut rngs {
        *lane = rng;
        rng = xorshift32_after(rng, 2 * strip as u64);
    }
    let mut hits = [NEVER; RECALL_LANES];
    for cycle in 0..strip as u32 {
        for (rng, hit) in rngs.iter_mut().zip(&mut hits) {
            xorshift32(rng);
            *hit = if xorshift32(rng) >= least { cycle } else { *hit };
        }
    }
    let tail = (first(RECALL_LANES)..end).filter(|_| {
        xorshift32(&mut rng);
        xorshift32(&mut rng) >= least
    });
    tail.last().or_else(|| (0..RECALL_LANES).rev().find(|&lane| hits[lane] != NEVER).map(|lane| first(lane) + i64::from(hits[lane])))
}

struct Wander {
    seed: u32,
    rng: u32,
    period: usize,
    counter: usize,
    glide: f32,
    level: f32,
    target: f32,
}

impl Wander {
    fn new(seed: u32, period: usize, glide: f32) -> Wander {
        Wander { seed, rng: seed, period: period.max(1), counter: 0, glide, level: 0.0, target: 0.0 }
    }

    fn place(&mut self, frames: i64, sparse: f32) {
        *self = Wander::new(self.seed, self.period, self.glide);
        if frames < 0 {
            self.counter = frames.unsigned_abs() as usize;
            return;
        }
        let (cycles, into) = (frames / self.period as i64, (frames % self.period as i64) as usize);
        let keep = f64::from(1.0 - self.glide);
        let whole = keep.powf(self.period as f64);
        let from = self.recalled(cycles, whole, sparse);
        self.rng = xorshift32_after(self.seed, draws(sparse) * from as u64);
        let mut level = 0.0;
        for _ in from..cycles {
            self.draw(sparse);
            level = f64::from(self.target) + (level - f64::from(self.target)) * whole;
        }
        if into > 0 {
            self.draw(sparse);
            level = f64::from(self.target) + (level - f64::from(self.target)) * keep.powf(into as f64);
            self.counter = self.period - into;
        }
        self.level = level as f32;
    }

    fn retime(&mut self, period: usize, glide: f32) {
        self.period = period.max(1);
        self.counter = self.counter.min(self.period);
        self.glide = glide;
    }

    fn recalled(&self, cycles: i64, whole: f64, sparse: f32) -> i64 {
        let kept = if whole == 0.0 {
            RECALL_LEAST
        } else if whole > 0.0 && whole < 1.0 {
            (FORGOTTEN / whole.ln()).ceil().max(RECALL_LEAST)
        } else {
            return 0;
        };
        if kept >= cycles as f64 {
            return 0;
        }
        let latest = cycles - kept as i64;
        let Some(least) = least_taken(sparse).filter(|_| sparse > 0.0) else {
            return latest;
        };
        let (mut end, mut reach) = (latest + 1, RECALL_REACH);
        while end > 0 {
            let start = (end - reach).max(0);
            if let Some(cycle) = last_taken(self.seed, least, start, end) {
                return cycle;
            }
            end = start;
            reach = (reach * 2).min(RECALL_FARTHEST);
        }
        latest
    }

    fn draw(&mut self, sparse: f32) {
        if let Some(drawn) = drawn(&mut self.rng, sparse) {
            self.target = drawn;
        }
    }

    fn tick(&mut self, sparse: f32) -> f32 {
        if self.counter == 0 {
            self.draw(sparse);
            self.counter = self.period;
        }
        self.counter -= 1;
        self.level += self.glide * (self.target - self.level);
        self.level
    }
}

struct Allpass {
    ring: Ring,
    delay: usize,
    gain: f32,
}

impl Allpass {
    fn tick(&mut self, x: f32) -> f32 {
        self.ring.allpass(self.delay, self.gain, x)
    }

    fn tick_scaled(&mut self, x: f32, scale: f32) -> f32 {
        self.ring.allpass(((self.delay as f32 * scale).round() as usize).max(1), self.gain * scale, x)
    }
}

#[derive(Clone, Copy)]
struct SplitCoeffs {
    low: BiquadCoeffs,
    high: BiquadCoeffs,
}

impl SplitCoeffs {
    fn at(hz: f64, sample_rate: f64) -> SplitCoeffs {
        let hz = hz.min(sample_rate * 0.45);
        SplitCoeffs { low: low_pass(hz, SPLIT_Q, sample_rate), high: high_pass(hz, SPLIT_Q, sample_rate) }
    }
}

impl PartialEq for SplitCoeffs {
    fn eq(&self, other: &SplitCoeffs) -> bool {
        same(&self.low, &other.low) && same(&self.high, &other.high)
    }
}

#[derive(Default)]
struct Split {
    low: [Biquad; 2],
    high: [Biquad; 2],
}

impl Split {
    fn tick(&mut self, x: f32, c: &SplitCoeffs) -> (f32, f32) {
        let x = f64::from(x);
        let low = self.low.iter_mut().fold(x, |x, stage| stage.run(c.low, x));
        let high = self.high.iter_mut().fold(x, |x, stage| stage.run(c.high, x));
        (low as f32, high as f32)
    }
}

#[derive(Default)]
struct Node {
    low: f32,
    high: f32,
    beside: f32,
    decay: f32,
    low_split: Split,
    high_split: Split,
    beside_split: [Biquad; 2],
}

impl Node {
    fn slowed(&mut self, decay: f32, lifting: bool, rise: f32) -> f32 {
        self.decay = if lifting && self.decay > 0.0 { decay.min(self.decay.max(f32::MIN_POSITIVE) * rise) } else { decay };
        self.decay
    }

    fn lifted(&mut self, x: f32, decay: f32, shelf: f32, shelf_high: f32, t: &Loop) -> (f32, f32) {
        let decay = self.slowed(decay, shelf.max(shelf_high) > 1.0, t.decay_rise);
        let most = 1.0 / decay;
        (self.tick(x, shelf.min(most), shelf_high.min(most), t), decay)
    }

    fn tick(&mut self, x: f32, shelf: f32, shelf_high: f32, t: &Loop) -> f32 {
        let (low_lift, high_lift) = ((shelf - 1.0).max(0.0), (shelf_high - 1.0).max(0.0));
        let (both_lift, high_excess) = (low_lift > 0.0 && high_lift > 0.0, (high_lift - low_lift).max(0.0));
        if t.steep {
            let (low, rest) = self.low_split.tick(x, &t.low_split);
            let shelved = shelf * low + rest;
            let beside = high_lift * rest + high_excess * low;
            let (rest, high) = self.high_split.tick(shelved, &t.high_split);
            let lifted = self.beside_split.iter_mut().fold(f64::from(beside), |x, stage| stage.run(t.high_split.high, x)) as f32;
            if both_lift { rest + high + lifted } else { rest + shelf_high * high }
        } else {
            self.low = one_pole_lp(x, t.cross, self.low);
            let shelved = shelved(x, self.low, shelf);
            self.high = one_pole_lp(shelved, t.high_cross, self.high);
            let beside = high_lift * (x - self.low) + high_excess * self.low;
            self.beside = one_pole_lp(beside, t.high_cross, self.beside);
            if both_lift { shelved + (beside - self.beside) } else { shelved_high(shelved, self.high, shelf_high) }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
struct Tap {
    delay: f32,
    level: f32,
    left: f32,
    right: f32,
    own: f32,
    far: f32,
}

impl Tap {
    fn pan(side: f64, cross: f64) -> (f32, f32, f32, f32) {
        let (right, left) = ((side as f32 + 1.0) * FRAC_PI_4 as f32).sin_cos();
        (left, right, (1.0 - cross as f32).sqrt(), (cross as f32).sqrt())
    }

    fn at(ms: f64, level: f64, (left, right, own, far): (f32, f32, f32, f32), sample_rate: f64) -> Tap {
        Tap { delay: (ms.clamp(0.0, params::Reverb::MAX_TAP_MS) * 0.001 * sample_rate) as f32, level: level as f32, left, right, own, far }
    }

    fn toward(&mut self, t: &Tap, a: f32) {
        self.delay += a * (t.delay - self.delay);
        self.level = eased(self.level, t.level, a);
        self.left += a * (t.left - self.left);
        self.right += a * (t.right - self.right);
        self.own += a * (t.own - self.own);
        self.far += a * (t.far - self.far);
    }

    fn settle(&mut self, t: &Tap) {
        if (t.far - self.far).abs() < WIDTH_SNAP {
            self.far = t.far;
            self.own = t.own;
        }
    }

    fn placed(&self, gain: f32, mid: f32, side: f32) -> (f32, f32) {
        if self.far == 0.0 {
            (gain * self.left * (mid + side), gain * self.right * (mid - side))
        } else {
            let (l, r) = (mid + side, mid - side);
            (gain * self.left * (self.own * l + self.far * r), gain * self.right * (self.own * r + self.far * l))
        }
    }
}

#[derive(Clone, Copy)]
struct Swing {
    turn: (f64, f64),
    step: (f64, f64),
    start: (f64, f64),
    turns: f64,
}

impl Swing {
    fn tick(&mut self) {
        let (c, s) = self.turn;
        self.turn = (c * self.step.0 - s * self.step.1, s * self.step.0 + c * self.step.1);
    }

    fn renormalize(&mut self) {
        let (c, s) = self.turn;
        let norm = (c * c + s * s).sqrt();
        if norm > 0.0 {
            self.turn = (c / norm, s / norm);
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Quarter {
    x: [f32; 2],
    y: [f32; 2],
}

impl Quarter {
    fn tick(&mut self, x: f32, a: f32) -> f32 {
        let y = a * a * (x + self.y[1]) - self.x[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

#[derive(Clone, Copy, Default)]
struct Quadrature {
    i: [Quarter; 4],
    q: [Quarter; 4],
    held: f32,
}

impl Quadrature {
    fn tick(&mut self, x: f32) -> (f32, f32) {
        let i = self.i.iter_mut().zip(QUADRATURE_I).fold(x, |x, (stage, a)| stage.tick(x, a));
        let q = self.q.iter_mut().zip(QUADRATURE_Q).fold(x, |x, (stage, a)| stage.tick(x, a));
        let late = self.held;
        self.held = q;
        (i, late)
    }
}

struct Turned {
    rings: [Ring; 2],
    quadrature: Quadrature,
    readers: [[Reader; PLATEAU_TAPS]; 2],
}

impl Turned {
    fn new(samples: usize) -> Turned {
        Turned { rings: [Ring::new(samples), Ring::new(samples)], quadrature: Quadrature::default(), readers: [[Reader::default(); PLATEAU_TAPS]; 2] }
    }

    fn clear(&mut self) {
        for ring in &mut self.rings {
            ring.clear();
        }
        self.quadrature = Quadrature::default();
        self.readers = [[Reader::default(); PLATEAU_TAPS]; 2];
    }

    fn push(&mut self, x: f32) {
        let (i, q) = self.quadrature.tick(x);
        self.rings[0].push(i);
        self.rings[1].push(q);
    }
}

struct Plateau {
    draws: [(f32, bool); PLATEAU_TAPS],
    copies: usize,
    shape: f32,
    at: [f32; PLATEAU_TAPS],
    gain: [f32; PLATEAU_TAPS],
    aim: [f32; PLATEAU_TAPS],
    swings: [Swing; PLATEAU_TAPS],
    normal: bool,
    rate: f64,
    glide: f32,
    direct: [Reader; 2],
}

impl Plateau {
    fn new(sample_rate: f64) -> Plateau {
        let mut rng = PLATEAU_SEED;
        let mut draws = [(0f32, false); PLATEAU_TAPS];
        for draw in &mut draws {
            let place = white(&mut rng) as f32;
            *draw = (place, white(&mut rng) >= 0.0);
        }
        let mut drift = DRIFT_SEED;
        let swings = std::array::from_fn(|_| {
            let hz = DRIFT_HZ.0 + (DRIFT_HZ.1 - DRIFT_HZ.0) * (0.5 + 0.5 * white(&mut drift));
            let way = if white(&mut drift) >= 0.0 { 1.0 } else { -1.0 };
            let phase = PI * white(&mut drift);
            Swing { turn: (phase.cos(), phase.sin()), step: (1.0, 0.0), start: (phase.cos(), phase.sin()), turns: way * hz / sample_rate }
        });
        let mut plateau = Plateau { draws, copies: 0, shape: 0.0, at: [0.0; PLATEAU_TAPS], gain: [0.0; PLATEAU_TAPS], aim: [0.0; PLATEAU_TAPS], swings, normal: false, rate: 0.0, glide: sdk::dsp::smoothing(SMOOTH_S, sample_rate) as f32, direct: [Reader::default(); 2] };
        plateau.lay(PLATEAU_TAPS, 0.0);
        plateau
    }

    fn retune(&mut self, rate: f64) {
        if rate == self.rate {
            return;
        }
        self.rate = rate;
        for swing in &mut self.swings {
            let (s, c) = (2.0 * PI * swing.turns * rate).sin_cos();
            swing.step = (c, s);
        }
    }

    fn lay(&mut self, copies: usize, shape: f32) {
        let copies = copies.clamp(1, PLATEAU_TAPS);
        if copies == self.copies && shape == self.shape {
            return;
        }
        let relaid = copies != self.copies;
        self.copies = copies;
        self.shape = shape;
        let scale = 1.0 / (copies as f32).sqrt();
        for (k, (at, aim)) in self.at.iter_mut().zip(&mut self.aim).enumerate().take(copies) {
            let (place, up) = self.draws[k];
            *at = (k as f32 + 0.5 + 0.5 * place) / copies as f32;
            *aim = if up { scale } else { -scale };
        }
        if shape != 0.0 {
            let weight = |at: f32| 1.0 + shape * (2.0 * at - 1.0);
            let mean = self.at.iter().take(copies).map(|at| weight(*at)).sum::<f32>() / copies as f32;
            for (at, aim) in self.at.iter().zip(&mut self.aim).take(copies) {
                *aim *= (weight(*at) / mean).sqrt();
            }
        }
        if relaid {
            self.gain = self.aim;
        }
    }

    fn settle(&mut self) {
        for (gain, aim) in self.gain.iter_mut().zip(&self.aim).take(self.copies) {
            if *gain != *aim {
                let next = *gain + self.glide * (*aim - *gain);
                *gain = if (next - *aim).abs() < PLATEAU_SETTLE { *aim } else { next };
            }
        }
    }

    fn place(&mut self, frames: i64) {
        for swing in &mut self.swings {
            let (s, c) = (2.0 * PI * (swing.turns * self.rate * frames as f64).fract()).sin_cos();
            let (c0, s0) = swing.start;
            swing.turn = (c0 * c - s0 * s, s0 * c + c0 * s);
        }
        self.normal = false;
    }

    fn renormalize(&mut self) {
        if self.normal {
            return;
        }
        let mut normal = true;
        for swing in &mut self.swings {
            let turn = swing.turn;
            swing.renormalize();
            normal &= swing.turn == turn;
        }
        self.normal = normal;
    }

    fn turn(&mut self) {
        for swing in self.swings.iter_mut().take(self.copies) {
            swing.tick();
        }
        self.normal = false;
    }

    fn tick(&self, pre: &Ring, base: f64, span: f64, carpet: f32) -> f32 {
        let direct = pre.linear(base);
        let spread: f32 = self.at.iter().zip(&self.gain).take(self.copies).map(|(at, gain)| pre.linear(base + f64::from(*at) * span) * gain).sum();
        direct + carpet * spread
    }

    fn turned(&mut self, channel: usize, pre: &Ring, turned: &mut Turned, base: f64, span: f64, carpet: f32, drifting: bool, how: Read) -> f32 {
        let direct = self.direct[channel].read(pre, base, how);
        let [ring_i, ring_q] = &turned.rings;
        let [readers_i, readers_q] = &mut turned.readers;
        let copies = self.at.iter().zip(&self.gain).zip(&self.swings).zip(readers_i.iter_mut().zip(readers_q.iter_mut())).take(self.copies);
        let spread: f32 = if drifting {
            copies.map(|(((at, gain), swing), (reader_i, reader_q))| {
                let at = base + f64::from(*at) * span;
                let (c, s) = swing.turn;
                (c as f32 * reader_i.read(ring_i, at, how) - s as f32 * reader_q.read(ring_q, at, how)) * gain
            }).sum()
        } else {
            copies.map(|(((at, gain), _), (reader, _))| reader.read(pre, base + f64::from(*at) * span, how) * gain).sum()
        };
        direct + carpet * spread
    }

    fn carpet(window: f32, fade: f32, seconds: f32, sample_rate: f32) -> f32 {
        let blend = (window / fade).min(1.0);
        blend * (2.0 * PLATEAU_LN_1000 * window / (seconds.max(params::Reverb::MIN_DECAY_S as f32) * sample_rate)).min(PLATEAU_MAX_CARPET).sqrt()
    }
}

fn carpet<const N: usize>(seed: u32, span: f64, first_now: bool) -> [(usize, f32); N] {
    let mut rng = seed;
    let scale = 1.0 / (N as f32).sqrt();
    let mut taps = [(0usize, 0f32); N];
    for (k, tap) in taps.iter_mut().enumerate() {
        let at = if first_now && k == 0 { 0.0 } else { (k as f64 + 0.5 + 0.5 * white(&mut rng)) / N as f64 * span };
        let sign = if k == 0 || white(&mut rng) >= 0.0 { scale } else { -scale };
        *tap = (at.round() as usize + 1, sign);
    }
    taps
}

fn spread(ring: &Ring, taps: &[(usize, f32)]) -> f32 {
    taps.iter().map(|(at, gain)| ring.tap(*at) * gain).sum()
}

struct Scatter {
    rings: [Ring; 2],
    taps: [[(usize, f32); SCATTER_TAPS]; 2],
    fed: bool,
}

impl Scatter {
    fn new(sample_rate: f64) -> Scatter {
        let span = SCATTER_MS * 0.001 * sample_rate;
        Scatter { rings: [Ring::new(span as usize + 8), Ring::new(span as usize + 8)], taps: SCATTER_SEEDS.map(|seed| carpet(seed, span, false)), fed: false }
    }

    fn clear(&mut self) {
        for ring in &mut self.rings {
            ring.clear();
        }
        self.fed = false;
    }

    fn rest(&mut self) {
        if self.fed {
            self.clear();
        }
    }

    fn tick(&mut self, (l, r): (f32, f32), amount: f32) -> (f32, f32) {
        if amount <= 0.0 {
            self.rest();
            return (l, r);
        }
        self.fed = true;
        self.rings[0].push(l);
        self.rings[1].push(r);
        let scattered_l = spread(&self.rings[0], &self.taps[0]);
        let scattered_r = spread(&self.rings[1], &self.taps[1]);
        (l + amount * (scattered_l - l), r + amount * (scattered_r - r))
    }
}

struct Shimmer {
    hp: Biquad,
    lp1: Biquad,
    lp2: Biquad,
    up: Halfband,
    down: Halfband,
    shifter: PitchShifter,
}

impl Shimmer {
    fn new(sample_rate: f64, seed: u32) -> Shimmer {
        Shimmer { hp: Biquad::default(), lp1: Biquad::default(), lp2: Biquad::default(), up: Halfband::default(), down: Halfband::default(), shifter: PitchShifter::new(sample_rate * 2.0, SHIMMER_GRAIN_MS, SHIMMER_JITTER_MS, seed) }
    }

    fn reset(&mut self) {
        self.hp.reset();
        self.lp1.reset();
        self.lp2.reset();
        self.up.reset();
        self.down.reset();
        self.shifter.reset();
    }

    fn tick(&mut self, x: f32, ratio: f32, low: BiquadCoeffs, high: BiquadCoeffs) -> f32 {
        self.hp.coeffs = low;
        self.lp1.coeffs = high;
        self.lp2.coeffs = high;
        let banded = self.lp2.tick(self.lp1.tick(self.hp.tick(f64::from(x)))) as f32;
        let (up0, up1) = self.up.upsample(banded);
        let shifted0 = self.shifter.process(up0, ratio);
        let shifted1 = self.shifter.process(up1, ratio);
        self.down.downsample(shifted0, shifted1)
    }
}

struct Loop {
    decay: f32,
    shelf: f32,
    shelf_high: f32,
    cross: f32,
    high_cross: f32,
    diffusion: f32,
    damp: f32,
    depth: f32,
    size: f32,
    gate: f32,
    quant: f32,
    hold: f32,
    dispersion: Dispersion,
    steep: bool,
    low_split: SplitCoeffs,
    high_split: SplitCoeffs,
    mixing: f32,
    sparse: f32,
    entry: f32,
    how: Read,
    build: Option<[f32; 3]>,
    even: Option<f32>,
    decay_rise: f32,
}

impl Loop {
    fn lost(&self, samples: f64) -> f32 {
        self.even.map_or(1.0, |rate| (f64::from(rate) * samples).exp() as f32)
    }
}

#[derive(Default)]
struct ArmReaders {
    modulated: Reader,
    delay1: Reader,
    decay: Reader,
    delay2: Reader,
    same: [Reader; 4],
    cross: [Reader; 3],
}

struct Arm {
    len: Geometry,
    modulated: Ring,
    delay1: Ring,
    decay: Ring,
    delay2: Ring,
    damp: f32,
    node: Node,
    node_out: Node,
    out: f32,
    wander: [Wander; 2],
    jitter: Wander,
    shimmer: Shimmer,
    disperser: Disperser,
    readers: ArmReaders,
}

impl Arm {
    fn new(geometry: &Geometry, scale: f64, sample_rate: f64) -> Arm {
        let len = geometry.scaled(scale);
        let sway = (params::Reverb::MAX_MOD_DEPTH_MS * sample_rate / 1000.0).ceil() as usize + WANDER_HEADROOM;
        let room = |base: f32, headroom: usize| Ring::new((f64::from(base) * params::Reverb::MAX_SIZE) as usize + headroom);
        let glide = dsp::smoothing(WANDER_GLIDE_S, sample_rate) as f32;
        Arm {
            len,
            modulated: room(len.modulated, sway),
            delay1: room(len.delay1, 4),
            decay: room(len.decay, 4),
            delay2: room(len.delay2, 4),
            damp: 0.0,
            node: Node::default(),
            node_out: Node::default(),
            out: 0.0,
            wander: geometry.wander.map(|(hz, seed)| Wander::new(seed, (sample_rate / hz) as usize, glide)),
            jitter: Wander::new(geometry.jitter_seed, sample_rate as usize, glide),
            shimmer: Shimmer::new(sample_rate, geometry.shimmer_seed),
            disperser: Disperser::default(),
            readers: ArmReaders::default(),
        }
    }

    fn clear(&mut self) {
        for ring in [&mut self.modulated, &mut self.delay1, &mut self.decay, &mut self.delay2] {
            ring.clear();
        }
        self.damp = 0.0;
        self.node = Node::default();
        self.node_out = Node::default();
        self.out = 0.0;
        self.shimmer.reset();
        self.disperser.reset();
        self.readers = ArmReaders::default();
    }

    fn place(&mut self, frames: i64, sparse: f32) {
        for wander in &mut self.wander {
            wander.place(frames, 0.0);
        }
        self.jitter.place(frames, sparse);
    }

    fn feed(&mut self, t: &Loop) -> f32 {
        let decay = if t.even.is_some() { t.lost(f64::from(self.len.delay2 * t.size)) } else { t.decay };
        let (shelved, decay) = self.node_out.lifted(self.out, decay, t.shelf, t.shelf_high, t);
        self.disperser.tick(decay * shelved, &t.dispersion)
    }

    fn resplit(&mut self) {
        self.node = Node::default();
        self.node_out = Node::default();
    }

    fn tick(&mut self, input: f32, fresh: f32, lfo: f32, random: f32, wandering: Option<f32>, t: &Loop) -> (f32, f32) {
        let len = self.len;
        let at = |base: f32| f64::from(base * t.size);
        let (same, cross) = if t.how == Read::Full {
            let r = &mut self.readers;
            (
                r.same[0].passed(&self.delay1, at(len.same[0])) + r.same[1].passed(&self.delay1, at(len.same[1])) - r.same[2].passed(&self.decay, at(len.same[2])) + r.same[3].passed(&self.delay2, at(len.same[3])),
                r.cross[0].passed(&self.delay1, at(len.cross[0])) + r.cross[1].passed(&self.decay, at(len.cross[1])) + r.cross[2].passed(&self.delay2, at(len.cross[2])),
            )
        } else {
            (
                self.delay1.linear(at(len.same[0])) + self.delay1.linear(at(len.same[1])) - self.decay.linear(at(len.same[2])) + self.delay2.linear(at(len.same[3])),
                self.delay1.linear(at(len.cross[0])) + self.decay.linear(at(len.cross[1])) + self.delay2.linear(at(len.cross[2])),
            )
        };
        let swing = t.depth.min(len.modulated * t.size * MOD_REACH);
        let jitter = self.jitter.tick(t.sparse);
        let sway = if random > 0.0 { (1.0 - random) * lfo + random * jitter } else { lfo };
        let wander = self.wander[0].tick(0.0) + self.wander[1].tick(0.0);
        let motion = sway * swing + wandering.map_or(0.0, |gain| gain * wander);
        let entered = t.build.map_or(input, |shares| input - (1.0 - shares[0]) * fresh);
        let swayed = f64::from(quantized(len.modulated * t.size + motion, t.quant));
        let modulated = self.readers.modulated.allpass(&mut self.modulated, swayed, G_MOD * t.mixing, entered, t.lost(swayed), t.how);
        let first_at = f64::from(quantized(len.delay1 * t.size, t.quant));
        let first = self.readers.delay1.read(&self.delay1, first_at, t.how);
        self.delay1.push(modulated);
        let decay = if t.even.is_some() { t.lost(first_at) } else { t.decay };
        let (shelved, decay) = self.node.lifted(first, decay, t.shelf, t.shelf_high, t);
        self.damp = one_pole_lp(shelved, t.damp, self.damp);
        let lingered = t.build.map_or(self.damp * decay, |shares| self.damp * decay + shares[1] * fresh);
        let decay_at = f64::from(quantized(len.decay * t.size, t.quant));
        let diffused = self.readers.decay.allpass(&mut self.decay, decay_at, t.diffusion, lingered, t.lost(decay_at), t.how);
        self.out = self.readers.delay2.read(&self.delay2, f64::from(quantized(len.delay2 * t.size, t.quant)), t.how);
        self.delay2.push(t.build.map_or(diffused, |shares| diffused + shares[2] * fresh));
        (same, cross)
    }
}

struct Line {
    ring: Ring,
    diffuser: Ring,
    length: f32,
    diffuser_delay: usize,
    gain: f32,
    shelf: f32,
    shelf_high: f32,
    shelf_damp: f32,
    damp: f32,
    node: Node,
    disperser: Disperser,
    reader: Reader,
    diffuser_loss: f32,
}

impl Line {
    fn new(index: usize, sample_rate: f64) -> Line {
        let length = LINE_MS[index] * sample_rate / 1000.0;
        let sway = (params::Reverb::MAX_MOD_DEPTH_MS * sample_rate / 1000.0).ceil() as usize + WANDER_HEADROOM;
        let diffuser_delay = (LINE_DIFFUSER_MS[index] * sample_rate / 1000.0).round() as usize;
        Line { ring: Ring::new((length * params::Reverb::MAX_SIZE) as usize + sway), diffuser: Ring::new(diffuser_delay + 4), length: length as f32, diffuser_delay, gain: 0.0, shelf: 1.0, shelf_high: 1.0, shelf_damp: 1.0, damp: 0.0, node: Node::default(), disperser: Disperser::default(), reader: Reader::default(), diffuser_loss: 1.0 }
    }

    fn clear(&mut self) {
        self.ring.clear();
        self.diffuser.clear();
        self.damp = 0.0;
        self.node = Node::default();
        self.disperser.reset();
        self.reader = Reader::default();
    }

    fn prepare(&mut self, seconds: f64, low_times: f64, high_times: f64, size: f32, hold: f32, disperse: f32, fade_db_per_s: f64, even: bool, sample_rate: f64) {
        let samples = f64::from(self.length * size) + self.diffuser_delay as f64 + f64::from(disperse) * DISPERSE_DELAY_MS * sample_rate / 1000.0;
        let lined = line_gain(seconds, sample_rate, samples);
        let decay = held(lined, hold);
        let low = held(line_gain(seconds * low_times, sample_rate, samples), hold);
        let high = held(line_gain(seconds * high_times, sample_rate, samples), hold);
        let damped = held(line_gain(seconds * DAMP_TIMES, sample_rate, samples), hold);
        self.gain = held(lined * fade_gain(fade_db_per_s, samples / sample_rate), hold);
        self.shelf = ratio(low, decay);
        self.shelf_high = ratio(high, decay);
        self.shelf_damp = ratio(damped, decay);
        if even {
            let inner = self.diffuser_delay as f64;
            let outer = samples - inner;
            self.gain = held(line_gain(seconds, sample_rate, outer) * fade_gain(fade_db_per_s, outer / sample_rate), hold);
            self.diffuser_loss = held(line_gain(seconds, sample_rate, inner) * fade_gain(fade_db_per_s, inner / sample_rate), hold);
        } else {
            self.diffuser_loss = 1.0;
        }
    }
}

struct Lattice {
    lines: [Line; LINES],
    jitter: [Wander; LINE_SWAYS],
    wander: [Wander; 2],
    entry: [Ring; 2],
    doors: [[(usize, f32); ENTRY_DOORS]; LINES],
    out: (f32, f32),
    prepared: Option<[u64; 9]>,
    rotated: Kept<1, (f32, f32)>,
}

impl Lattice {
    fn new(sample_rate: f64) -> Lattice {
        let glide = dsp::smoothing(WANDER_GLIDE_S, sample_rate) as f32;
        let span = ENTRY_MS * 0.001 * sample_rate;
        Lattice {
            lines: std::array::from_fn(|index| Line::new(index, sample_rate)),
            jitter: std::array::from_fn(|index| Wander::new(LINE_JITTER_SEED.wrapping_add(index as u32), sample_rate as usize, glide)),
            wander: LINE_WANDER.map(|(hz, seed)| Wander::new(seed, (sample_rate / hz) as usize, glide)),
            entry: [Ring::new(span as usize + 8), Ring::new(span as usize + 8)],
            doors: std::array::from_fn(|index| carpet(ENTRY_SEED.wrapping_add(index as u32), span, true)),
            out: (0.0, 0.0),
            prepared: None,
            rotated: None,
        }
    }

    fn clear(&mut self) {
        for line in &mut self.lines {
            line.clear();
        }
        for ring in &mut self.entry {
            ring.clear();
        }
        self.out = (0.0, 0.0);
    }

    fn place(&mut self, frames: i64, sparse: f32) {
        for wander in &mut self.wander {
            wander.place(frames, 0.0);
        }
        for jitter in &mut self.jitter {
            jitter.place(frames, sparse);
        }
    }

    fn retime(&mut self, period: usize, glide: f32) {
        for jitter in &mut self.jitter {
            jitter.retime(period, glide);
        }
    }

    fn prepare(&mut self, seconds: f64, low_times: f64, high_times: f64, diffusion: f32, size: f32, hold: f32, disperse: f32, fade_db_per_s: f64, even: bool, sample_rate: f64) {
        let inputs = Some([seconds, low_times, high_times, f64::from(diffusion), f64::from(size), f64::from(hold), f64::from(disperse), fade_db_per_s, f64::from(u8::from(even))].map(f64::to_bits));
        if inputs == self.prepared {
            return;
        }
        self.prepared = inputs;
        let stretched = if even { seconds } else { seconds * (LATTICE_STRETCH + LATTICE_STRETCH_PER_DIFFUSION * f64::from(diffusion)) };
        for line in &mut self.lines {
            line.prepare(stretched, low_times, high_times, size, hold, disperse, fade_db_per_s, even, sample_rate);
        }
    }

    fn tick(&mut self, mid: f32, side: f32, front: (f32, f32), front_level: f32, inject: (f32, f32), far: Option<(f32, f32)>, lfo: (f32, f32), random: f32, wandering: Option<f32>, t: &Loop) -> (f32, f32) {
        let wander = self.wander[0].tick(0.0) + self.wander[1].tick(0.0);
        let slow = wandering.map_or(0.0, |gain| gain * wander);
        self.entry[0].push(mid);
        self.entry[1].push(side);
        let mut read = [0f32; LINES];
        let (mut left, mut right) = (0.0, 0.0);
        for (index, line) in self.lines.iter_mut().enumerate() {
            let base = line.length * t.size;
            let motion = if index % 2 == 1 {
                let sway = index / 2;
                let shape = [lfo.0, lfo.1, -lfo.0, -lfo.1][sway % 4];
                let swing = t.depth.min(base * MOD_REACH);
                let jitter = self.jitter[sway].tick(t.sparse);
                let jittered = if random > 0.0 { (1.0 - random) * shape + random * jitter } else { shape };
                jittered * swing + slow
            } else {
                0.0
            };
            let tapped = line.reader.read(&line.ring, f64::from(quantized(base + motion, t.quant)), t.how);
            let (shelved, gain) = line.node.lifted(tapped, line.gain, line.shelf, line.shelf_high, t);
            line.damp = one_pole_lp(shelved, t.damp, line.damp);
            let passed = line.disperser.tick(gain * shelved_high(shelved, line.damp, line.shelf_damp), &t.dispersion);
            read[index] = passed;
            if LINE_SIDES[index] > 0.0 {
                left += LINE_SIGNS[index] * passed;
            } else {
                right += LINE_SIGNS[index] * passed;
            }
        }
        if t.mixing >= 1.0 {
            hadamard(&mut read);
        } else {
            butterfly(&mut read, kept(&mut self.rotated, [f64::from(t.mixing)], || rotation(t.mixing)));
        }
        for (index, line) in self.lines.iter_mut().enumerate() {
            let injected = if LINE_SIDES[index] > 0.0 { inject.0 } else { inject.1 };
            let mixed = LINE_FLIPS[index] * read[(index + LINE_TURN) % LINES];
            let (mid, side) = if t.entry > 0.0 {
                let doors = &self.doors[index];
                (mid + t.entry * (spread(&self.entry[0], doors) - mid), side + t.entry * (spread(&self.entry[1], doors) - side))
            } else {
                (mid, side)
            };
            let fed = t.gate * LINE_SIGNS[index] * (mid + side * LINE_SIDES[index] + injected) + mixed;
            let fed = match far {
                Some((far_mid, far_side)) => fed + t.gate * LINE_SIGNS[index] * (far_mid - far_side * LINE_SIDES[index]),
                None => fed,
            };
            let diffused = if t.even.is_some() { lossy_allpass(&mut line.diffuser, line.diffuser_delay, t.diffusion, fed, line.diffuser_loss) } else { line.diffuser.allpass(line.diffuser_delay, t.diffusion, fed) };
            line.ring.push(diffused);
        }
        self.out = (LATTICE_OUT * left + front_level * front.0, LATTICE_OUT * right + front_level * front.1);
        self.out
    }
}

#[derive(Default)]
struct Channel([State; FILTERS]);

impl Channel {
    fn tick(&mut self, x: f32, z: &Controls) -> f32 {
        self.0.iter_mut().zip(&z.paths).zip(&z.filters).fold(x, |y, ((state, path), coeffs)| state.tick(y, *path, coeffs))
    }
}

struct Input {
    pre: Ring,
    bloom: [Ring; ATTACK_SHARES.len()],
    dc: DcBlock,
    diffuser: [Allpass; 4],
    reader: Reader,
    bloom_readers: [Reader; ATTACK_SHARES.len()],
    turned: Turned,
}

impl Input {
    fn new(sample_rate: f64, scale: f64) -> Input {
        let pre = ((sample_rate * (params::Reverb::MAX_PRE_DELAY_MS * 0.001 + params::Reverb::MAX_EARLY_SPAN_MS.max(params::Reverb::MAX_PLATEAU_MS) * 0.001)).round() as usize + 8).max(64);
        let tapped = (sample_rate * params::Reverb::MAX_TAP_MS * 0.001).ceil() as usize + 8;
        Input {
            pre: Ring::new(pre.max(tapped)),
            bloom: ATTACK_SHARES.map(|share| Ring::new((share * params::Reverb::MAX_ATTACK_MS * 0.001 * sample_rate).ceil() as usize + 4)),
            dc: DcBlock::default(),
            diffuser: DIFFUSER.map(|(base, gain)| {
                let delay = (base as f64 * scale).round() as usize;
                Allpass { ring: Ring::new((delay + 4).max(32)), delay, gain }
            }),
            reader: Reader::default(),
            bloom_readers: [Reader::default(); ATTACK_SHARES.len()],
            turned: Turned::new(pre),
        }
    }

    fn clear(&mut self) {
        self.pre.clear();
        self.clear_bloom();
        for allpass in &mut self.diffuser {
            allpass.ring.clear();
        }
        self.dc = DcBlock::default();
        self.reader = Reader::default();
        self.turned.clear();
    }

    fn clear_bloom(&mut self) {
        for ring in &mut self.bloom {
            ring.clear();
        }
        self.bloom_readers = [Reader::default(); ATTACK_SHARES.len()];
    }

    fn bloomed(&mut self, delayed: f32, attack: f32, gain: f32, full: bool) -> f32 {
        if full {
            return ATTACK_SHARES.iter().zip(&mut self.bloom).zip(&mut self.bloom_readers).fold(delayed, |x, ((share, ring), reader)| reader.allpass(ring, (share * f64::from(attack)).max(1.0), gain, x, 1.0, Read::Full));
        }
        ATTACK_SHARES.iter().zip(&mut self.bloom).fold(delayed, |x, (share, ring)| ring.allpass_linear((share * f64::from(attack)).max(1.0), gain, x))
    }

    fn delayed(&mut self, at: f64, how: Read) -> f32 {
        self.reader.read(&self.pre, at, how)
    }

    fn diffused(&mut self, x: f32, dc_pole: f32, tickle: f32, spread: f32) -> f32 {
        let blocked = self.dc.tick(x, dc_pole);
        self.diffuser.iter_mut().fold(blocked + tickle, |x, allpass| if spread == 1.0 { allpass.tick(x) } else { allpass.tick_scaled(x, spread) })
    }
}

#[derive(Default)]
struct Derived {
    decays: Kept<4, (f32, f32, f32)>,
    crossovers: Kept<2, (f32, f32, SplitCoeffs, SplitCoeffs)>,
    damps: Kept<2, f32>,
    ratios: Kept<1, f32>,
    pans: [Kept<2, (f32, f32, f32, f32)>; TAPS],
}

#[derive(Clone, Copy, PartialEq)]
struct Controls {
    pre_delay: f32,
    attack: f32,
    early: f32,
    span: f32,
    early_width: f32,
    stereo: f32,
    spread: f32,
    front: f32,
    shape: f32,
    disperse: f32,
    decay: f32,
    bass: f32,
    treble: f32,
    seconds: f32,
    low_times: f32,
    high_times: f32,
    cross: f32,
    high_cross: f32,
    diffusion: f32,
    size: f32,
    damp: f32,
    depth: f32,
    random: f32,
    filters: [Coeffs; FILTERS],
    paths: [Path; FILTERS],
    width: f32,
    mix: f32,
    keep: f32,
    forward: f32,
    shimmer: f32,
    ratio: f32,
    freeze: f64,
    ducking: f32,
    taps: [Tap; TAPS],
    scatter: f32,
    cubic: bool,
    steep: bool,
    low_split: SplitCoeffs,
    high_split: SplitCoeffs,
    mixing: f32,
    plateau: f32,
    sparse: f32,
    tap_scatter: f32,
    entry: f32,
    cross_feed: f32,
    early_cross: f32,
    cross_at: f32,
    build: f32,
    full: bool,
    even: bool,
}

fn opened(hz: f64, full: bool, sample_rate: f64) -> f32 {
    if full && hz >= params::Reverb::OPEN_HZ { 0.0 } else { lp_coeff(hz, sample_rate) }
}

impl Controls {
    fn targets(p: &params::Reverb, filters: &[Tuned; FILTERS], wet: f32, clock: &Clock, sample_rate: f64, loop_samples: f64, derived: &mut Derived) -> Controls {
        let (decay, bass, treble) = kept(&mut derived.decays, [p.decay_s, p.low_decay, p.high_decay, loop_samples], || (decay_gain(p.decay_s, sample_rate, loop_samples), decay_gain(p.decay_s * p.low_decay, sample_rate, loop_samples), decay_gain(p.decay_s * p.high_decay, sample_rate, loop_samples)));
        let (cross, high_cross, low_split, high_split) = kept(&mut derived.crossovers, [p.low_crossover_hz, p.high_crossover_hz], || (lp_coeff(p.low_crossover_hz, sample_rate), lp_coeff(p.high_crossover_hz, sample_rate), SplitCoeffs::at(p.low_crossover_hz, sample_rate), SplitCoeffs::at(p.high_crossover_hz, sample_rate)));
        let damp = kept(&mut derived.damps, [p.damping_hz, f64::from(u8::from(p.full_band))], || opened(p.damping_hz, p.full_band, sample_rate));
        let ratio = kept(&mut derived.ratios, [p.shift], || (p.shift / 12.0).exp2() as f32);
        let taps = p.taps();
        Controls {
            pre_delay: (p.pre_delay_ms * 0.001 * sample_rate) as f32,
            attack: (p.attack_ms * 0.001 * sample_rate) as f32,
            early: p.early_level as f32,
            span: (p.early_span_ms * 0.001 * sample_rate) as f32,
            early_width: p.early_width as f32,
            stereo: p.stereo as f32,
            spread: p.input_diffusion as f32,
            front: p.hall_front as f32,
            shape: p.attack_shape as f32,
            disperse: p.disperse as f32,
            decay,
            bass,
            treble,
            seconds: p.decay_s as f32,
            low_times: p.low_decay as f32,
            high_times: p.high_decay as f32,
            cross,
            high_cross,
            diffusion: p.diffusion as f32,
            size: p.size as f32,
            damp,
            depth: (p.mod_depth * (sample_rate / 1000.0)) as f32,
            random: p.mod_random as f32,
            filters: filters.map(|tuned| tuned.coeffs),
            paths: filters.map(|tuned| tuned.path),
            width: p.width as f32,
            mix: wet,
            keep: p.dry as f32,
            forward: p.forward as f32,
            shimmer: p.shimmer as f32,
            ratio,
            freeze: if p.freeze { 1.0 } else { 0.0 },
            ducking: p.ducking as f32,
            taps: std::array::from_fn(|index| {
                let (ms, synced, level, side, cross) = taps[index];
                Tap::at(synced.map_or(ms, |division| clock.ms(division)), level, kept(&mut derived.pans[index], [side, cross], || Tap::pan(side, cross)), sample_rate)
            }),
            scatter: p.early_scatter as f32,
            cubic: p.cubic_lines,
            steep: p.steep_crossovers,
            low_split,
            high_split,
            mixing: p.mixing as f32,
            plateau: (p.plateau_ms * 0.001 * sample_rate) as f32,
            sparse: p.mod_sparse as f32,
            tap_scatter: p.tap_scatter as f32,
            entry: p.entry_spread as f32,
            cross_feed: p.cross_feed as f32,
            early_cross: p.early_cross as f32,
            cross_at: (p.cross_ms * 0.001 * sample_rate) as f32,
            build: p.build as f32,
            full: p.full_band,
            even: p.even_decay,
        }
    }

    fn toward(&mut self, t: &Controls, a: f32) {
        self.pre_delay += a * (t.pre_delay - self.pre_delay);
        self.attack += a * (t.attack - self.attack);
        self.early = eased(self.early, t.early, a);
        self.span += a * (t.span - self.span);
        self.early_width += a * (t.early_width - self.early_width);
        self.stereo += a * (t.stereo - self.stereo);
        self.spread += a * (t.spread - self.spread);
        self.front += a * (t.front - self.front);
        self.shape += a * (t.shape - self.shape);
        self.disperse += a * (t.disperse - self.disperse);
        self.decay += a * (t.decay - self.decay);
        self.bass += a * (t.bass - self.bass);
        self.treble += a * (t.treble - self.treble);
        self.seconds += a * (t.seconds - self.seconds);
        self.low_times += a * (t.low_times - self.low_times);
        self.high_times += a * (t.high_times - self.high_times);
        self.cross += a * (t.cross - self.cross);
        self.high_cross += a * (t.high_cross - self.high_cross);
        self.diffusion += a * (t.diffusion - self.diffusion);
        self.size += a * (t.size - self.size);
        self.damp += a * (t.damp - self.damp);
        self.depth = eased(self.depth, t.depth, a);
        self.random += a * (t.random - self.random);
        for (mine, theirs) in self.filters.iter_mut().zip(&t.filters) {
            mine.approach(theirs, a);
        }
        self.paths = t.paths;
        self.width += a * (t.width - self.width);
        self.mix += a * (t.mix - self.mix);
        self.keep += a * (t.keep - self.keep);
        self.forward += a * (t.forward - self.forward);
        self.ducking += a * (t.ducking - self.ducking);
        self.shimmer += a * (t.shimmer - self.shimmer);
        self.ratio += a * (t.ratio - self.ratio);
        self.freeze += f64::from(a) * (t.freeze - self.freeze);
        for (tap, target) in self.taps.iter_mut().zip(&t.taps) {
            tap.toward(target, a);
        }
        self.scatter += a * (t.scatter - self.scatter);
        self.cubic = t.cubic;
        self.steep = t.steep;
        self.low_split = t.low_split;
        self.high_split = t.high_split;
        self.mixing += a * (t.mixing - self.mixing);
        self.plateau += a * (t.plateau - self.plateau);
        self.sparse += a * (t.sparse - self.sparse);
        self.tap_scatter += a * (t.tap_scatter - self.tap_scatter);
        self.entry += a * (t.entry - self.entry);
        self.cross_feed += a * (t.cross_feed - self.cross_feed);
        self.early_cross += a * (t.early_cross - self.early_cross);
        self.cross_at += a * (t.cross_at - self.cross_at);
        self.build += a * (t.build - self.build);
        self.full = t.full;
        self.even = t.even;
    }

    fn settle(&mut self, t: &Controls) {
        if (t.attack - self.attack).abs() < ATTACK_SNAP {
            self.attack = t.attack;
        }
        if (t.early_width - self.early_width).abs() < WIDTH_SNAP {
            self.early_width = t.early_width;
        }
        if (t.stereo - self.stereo).abs() < WIDTH_SNAP {
            self.stereo = t.stereo;
        }
        if (t.random - self.random).abs() < WIDTH_SNAP {
            self.random = t.random;
        }
        if (t.spread - self.spread).abs() < WIDTH_SNAP {
            self.spread = t.spread;
        }
        if (t.shape - self.shape).abs() < WIDTH_SNAP {
            self.shape = t.shape;
        }
        if (t.disperse - self.disperse).abs() < WIDTH_SNAP {
            self.disperse = t.disperse;
        }
        if (t.scatter - self.scatter).abs() < WIDTH_SNAP {
            self.scatter = t.scatter;
        }
        if (t.mixing - self.mixing).abs() < WIDTH_SNAP {
            self.mixing = t.mixing;
        }
        if (t.plateau - self.plateau).abs() < ATTACK_SNAP {
            self.plateau = t.plateau;
        }
        if (t.sparse - self.sparse).abs() < WIDTH_SNAP {
            self.sparse = t.sparse;
        }
        if (t.tap_scatter - self.tap_scatter).abs() < WIDTH_SNAP {
            self.tap_scatter = t.tap_scatter;
        }
        if (t.entry - self.entry).abs() < WIDTH_SNAP {
            self.entry = t.entry;
        }
        if (t.cross_feed - self.cross_feed).abs() < WIDTH_SNAP {
            self.cross_feed = t.cross_feed;
        }
        if (t.early_cross - self.early_cross).abs() < WIDTH_SNAP {
            self.early_cross = t.early_cross;
        }
        if (t.build - self.build).abs() < WIDTH_SNAP {
            self.build = t.build;
        }
        for (tap, target) in self.taps.iter_mut().zip(&t.taps) {
            tap.settle(target);
        }
    }

    fn shares(&self) -> Option<[f32; 3]> {
        (self.build > 0.0).then(|| {
            let later = (0.5 * self.build).sqrt();
            [(1.0 - self.build).max(0.0).sqrt(), later, later]
        })
    }

    fn even_rate(&self, decay: f32, loop_base: f32, size: f32) -> Option<f32> {
        self.even.then(|| (PLATE_DECAY_STAGES * f64::from(decay).ln() / f64::from(loop_base * size).max(1.0)) as f32)
    }

    fn tank(&self, freezing: bool, latched_size: &mut f32, dispersion: Dispersion, fade: f32, loop_base: f32, decay_rise: f32) -> Loop {
        let how = Read::of(self.cubic, self.full);
        if !freezing {
            *latched_size = self.size;
            let decay = self.decay * fade;
            return Loop { decay, shelf: ratio(self.bass, self.decay), shelf_high: ratio(self.treble, self.decay), cross: self.cross, high_cross: self.high_cross, diffusion: self.diffusion, damp: self.damp, depth: self.depth, size: self.size, gate: 1.0, quant: 0.0, hold: 0.0, dispersion, steep: self.steep, low_split: self.low_split, high_split: self.high_split, mixing: self.mixing, sparse: self.sparse, entry: self.entry, how, build: self.shares(), even: self.even_rate(decay, loop_base, self.size), decay_rise };
        }
        let f = self.freeze as f32;
        let decay = held(self.decay, f);
        let size = self.size + f * (*latched_size - self.size);
        Loop { decay, shelf: ratio(held(self.bass, f), decay), shelf_high: ratio(held(self.treble, f), decay), cross: self.cross, high_cross: self.high_cross, diffusion: self.diffusion, damp: self.damp * (1.0 - f), depth: self.depth * (1.0 - f), size, gate: 1.0 - f, quant: f, hold: f, dispersion, steep: self.steep, low_split: self.low_split, high_split: self.high_split, mixing: self.mixing, sparse: self.sparse, entry: self.entry, how, build: self.shares(), even: self.even_rate(decay, loop_base, size), decay_rise }
    }
}

pub struct Reverb {
    sample_rate: f64,
    smoothing: f32,
    detect_attack: f32,
    detect_release: f32,
    plateau_fade: f32,
    dc_pole: f32,
    decay_rise: f32,
    loop_base: f32,
    mid: Input,
    side: Input,
    plateau: Plateau,
    blooming: bool,
    spreading: bool,
    tank: Tank,
    a: Arm,
    b: Arm,
    lattice: Lattice,
    disperse_coeffs: BiquadCoeffs,
    dispersion: Kept<2, Dispersion>,
    age_poles: [f32; 2],
    age_mid: f32,
    age_side: f32,
    age_hold: (f32, f32),
    age_out: (f32, f32),
    age_every: usize,
    age_count: usize,
    loop_low: BiquadCoeffs,
    loop_high: BiquadCoeffs,
    filters: [Tuned; FILTERS],
    left: Channel,
    right: Channel,
    early_left: Channel,
    early_right: Channel,
    tap_left: Channel,
    tap_right: Channel,
    early_scatter: Scatter,
    front_scatter: Scatter,
    tap_scatter: Scatter,
    early_far: Delayed,
    tap_far: Delayed,
    tank_far: Delayed,
    early_readers: [[Reader; EARLY_TAPS]; 4],
    tap_readers: [[Reader; 2]; TAPS],
    steep: bool,
    controls: Option<Controls>,
    derived: Derived,
    frozen_size: f32,
    phase: f64,
    rate: Kept<1, f64>,
    restart: bool,
    tickle: f32,
    duck_env: f32,
    duck_active: f32,
    duck: Kept<3, (f32, f32, f32)>,
    shimmered: bool,
    followed: f32,
    since_peak: u64,
    fades: Kept<3, (f32, f32)>,
    until_hop: usize,
    held: Option<Controls>,
    held_since_peak: u64,
    held_frozen_size: f32,
    frames: i64,
    adrift: bool,
    drifted: bool,
    silence: Silence,
}

impl Reverb {
    fn rest(&mut self) {
        self.clear();
        self.plateau = Plateau::new(self.sample_rate);
        (self.blooming, self.spreading, self.shimmered) = (false, false, false);
        self.controls = None;
        (self.age_count, self.since_peak) = (0, 0);
        (self.tickle, self.duck_env, self.duck_active) = (1.0, 0.0, 0.0);
    }

    fn clear(&mut self) {
        self.mid.clear();
        self.side.clear();
        self.clear_tanks();
        self.left = Channel::default();
        self.early_left = Channel::default();
        self.early_right = Channel::default();
        self.right = Channel::default();
        self.tap_left = Channel::default();
        self.tap_right = Channel::default();
        self.early_scatter.clear();
        self.front_scatter.clear();
        self.tap_scatter.clear();
        self.early_far.clear();
        self.tap_far.clear();
        self.tank_far.clear();
        self.early_readers = [[Reader::default(); EARLY_TAPS]; 4];
        self.tap_readers = [[Reader::default(); 2]; TAPS];
        self.age_mid = 0.0;
        self.age_side = 0.0;
        self.age_hold = (0.0, 0.0);
        self.age_out = (0.0, 0.0);
        self.followed = 0.0;
    }

    fn clear_tanks(&mut self) {
        self.a.clear();
        self.b.clear();
        self.lattice.clear();
    }
}

impl Unit for Reverb {
    type Params = params::Reverb;

    fn new(sample_rate: f64, _: usize) -> Reverb {
        let scale = sample_rate / FS0;
        let a = Arm::new(&ARM_A, scale, sample_rate);
        let b = Arm::new(&ARM_B, scale, sample_rate);
        let loop_base = a.len.total() + b.len.total();
        let defaults = params::Reverb::DEFAULT;
        Reverb {
            sample_rate,
            smoothing: dsp::smoothing(SMOOTH_S, sample_rate) as f32,
            detect_attack: dsp::smoothing(DUCK_DETECT_ATTACK_S, sample_rate) as f32,
            detect_release: dsp::smoothing(DUCK_DETECT_RELEASE_S, sample_rate) as f32,
            plateau_fade: (PLATEAU_FADE_MS * 0.001 * sample_rate) as f32,
            dc_pole: lp_coeff(DC_CUTOFF_HZ, sample_rate),
            decay_rise: 1.0 / lp_coeff(DECAY_RISE_HZ, sample_rate),
            loop_base,
            mid: Input::new(sample_rate, scale),
            side: Input::new(sample_rate, scale),
            plateau: Plateau::new(sample_rate),
            blooming: false,
            spreading: false,
            tank: defaults.tank,
            a,
            b,
            lattice: Lattice::new(sample_rate),
            disperse_coeffs: all_pass(DISPERSE_HZ, DISPERSE_Q, sample_rate),
            dispersion: None,
            age_poles: AGE_BANDWIDTH_HZ.map(|hz| lp_coeff(hz, sample_rate)),
            age_mid: 0.0,
            age_side: 0.0,
            age_hold: (0.0, 0.0),
            age_out: (0.0, 0.0),
            age_every: ((sample_rate / AGE_HOLD_HZ).round() as usize).max(1),
            age_count: 0,
            loop_low: high_pass(SHIMMER_LOW_HZ, SHIMMER_Q, sample_rate),
            loop_high: low_pass(SHIMMER_HIGH_HZ.min(sample_rate * 0.45), SHIMMER_Q, sample_rate),
            filters: defaults.filters().map(|filter| Tuned::of(&filter, sample_rate)),
            left: Channel::default(),
            right: Channel::default(),
            early_left: Channel::default(),
            early_right: Channel::default(),
            tap_left: Channel::default(),
            tap_right: Channel::default(),
            early_scatter: Scatter::new(sample_rate),
            front_scatter: Scatter::new(sample_rate),
            tap_scatter: Scatter::new(sample_rate),
            early_far: Delayed::new(sample_rate),
            tap_far: Delayed::new(sample_rate),
            tank_far: Delayed::new(sample_rate),
            early_readers: [[Reader::default(); EARLY_TAPS]; 4],
            tap_readers: [[Reader::default(); 2]; TAPS],
            steep: defaults.steep_crossovers,
            controls: None,
            derived: Derived::default(),
            frozen_size: defaults.size as f32,
            phase: 0.0,
            rate: None,
            restart: true,
            tickle: 1.0,
            duck_env: 0.0,
            duck_active: 0.0,
            duck: None,
            shimmered: false,
            followed: 0.0,
            since_peak: 0,
            fades: None,
            until_hop: 0,
            held: None,
            held_since_peak: 0,
            held_frozen_size: defaults.size as f32,
            frames: 0,
            adrift: true,
            drifted: false,
            silence: Silence::default(),
        }
    }

    fn restart(&mut self) {
        self.restart = true;
        if self.silence.lasting(self.sample_rate) && !self.holding() {
            self.rest();
        }
    }

    fn holding(&self) -> bool {
        self.controls.is_some_and(|z| z.freeze > f64::from(FREEZE_EPS))
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], params: &params::Reverb, wet: f32, clock: &Clock) {
        dsp::sanitize(l, r);
        let quiet = Silence::trailing(l, r);
        if self.restart {
            self.restart = false;
            self.until_hop = 0;
            self.begin(params, clock);
        }
        let n = l.len().min(r.len());
        let mut done = 0;
        while done < n {
            if self.until_hop == 0 {
                self.held = self.controls;
                self.held_since_peak = self.since_peak;
                self.held_frozen_size = self.frozen_size;
                self.plateau.renormalize();
                self.until_hop = HOP;
            }
            let span = (n - done).min(self.until_hop);
            let clock = Clock { beat: clock.beat.map(|beat| Beat { since: beat.since + done as i64, ..beat }), ..*clock };
            self.span(&mut l[done..done + span], &mut r[done..done + span], params, wet, &clock);
            self.until_hop -= span;
            done += span;
        }
        self.silence.passed(quiet.min(Silence::trailing(l, r)), n);
    }
}

impl Reverb {
    fn begin(&mut self, params: &params::Reverb, clock: &Clock) {
        self.phase = begun(params.start_phase, turns(clock, params.mod_sync, params.mod_division, params.mod_rate_hz)).1;
        self.frames = clock.beat.map_or(0, |beat| (beat.at(0) * beat.frames_per_beat).round() as i64);
        self.adrift = true;
    }

    fn span(&mut self, l: &mut [f32], r: &mut [f32], params: &params::Reverb, wet: f32, clock: &Clock) {
        let sample_rate = self.sample_rate;
        if params.tank != self.tank {
            self.tank = params.tank;
            self.clear_tanks();
            self.adrift = true;
        }
        if params.steep_crossovers != self.steep {
            self.steep = params.steep_crossovers;
            self.a.resplit();
            self.b.resplit();
            for line in &mut self.lattice.lines {
                line.node = Node::default();
            }
        }
        let size = self.held.map_or(params.size, |held| f64::from(held.size));
        for (tuned, filter) in self.filters.iter_mut().zip(params.filters()) {
            tuned.tune(&filter, sample_rate);
        }
        let target = Controls::targets(params, &self.filters, wet, clock, sample_rate, f64::from(self.loop_base) * size, &mut self.derived);
        let rate_hz = clock.rate(params.mod_sync, params.mod_division, params.mod_rate_hz);
        let step = kept(&mut self.rate, [rate_hz], || {
            let period = (sample_rate / rate_hz.max(1e-3)) as usize;
            let glide = dsp::smoothing(JITTER_GLIDE_TURNS / rate_hz.max(1e-3), sample_rate) as f32;
            self.a.jitter.retime(period, glide);
            self.b.jitter.retime(period, glide);
            self.lattice.retime(period, glide);
            rate_hz / sample_rate
        });
        let a = self.smoothing;
        let (detect_attack, detect_release) = (self.detect_attack, self.detect_release);
        let (duck_attack, duck_release, threshold) = kept(&mut self.duck, [params.duck_attack_ms, params.duck_release_ms, params.threshold_db], || (dsp::smoothing(params.duck_attack_ms * 0.001, sample_rate) as f32, dsp::smoothing(params.duck_release_ms * 0.001, sample_rate) as f32, dsp::db_to_gain(params.threshold_db) as f32));
        if self.controls.is_none() {
            self.frozen_size = target.size;
            self.held_frozen_size = target.size;
        }
        let mut z = self.controls.unwrap_or(target);
        let held = self.held.unwrap_or(target);
        let tapping = params.tapping() || held.taps.iter().any(|tap| tap.level > 0.0);
        self.plateau.lay(params.plateau_copies, params.plateau_shape as f32);
        self.plateau.retune(params.plateau_drift);
        let drifting = params.plateau_drift > 0.0;
        let plateauing = held.plateau > 0.0 || target.plateau > 0.0;
        if self.adrift {
            self.a.place(self.frames, target.sparse);
            self.b.place(self.frames, target.sparse);
            self.lattice.place(self.frames, target.sparse);
        }
        if self.adrift || (plateauing && drifting && !self.drifted) {
            self.plateau.place(self.frames);
        }
        self.adrift = false;
        self.drifted = plateauing && drifting;
        let plateau_fade = self.plateau_fade;
        let mut frozen_size = self.frozen_size;
        let dispersed = held.disperse > 0.0 || target.disperse > 0.0;
        let dispersion = kept(&mut self.dispersion, [f64::from(held.disperse), f64::from(u8::from(dispersed))], || {
            let dispersion = Dispersion::at(held.disperse, dispersed, self.disperse_coeffs, sample_rate);
            self.a.disperser.settle(&dispersion);
            self.b.disperser.settle(&dispersion);
            for line in &mut self.lattice.lines {
                line.disperser.settle(&dispersion);
            }
            dispersion
        });
        if params.age == Age::Now {
            self.age_mid = 0.0;
            self.age_side = 0.0;
            self.age_hold = (0.0, 0.0);
            self.age_out = (0.0, 0.0);
        }
        let fade_db_per_s = if params.fade_after_s > 0.0 && self.held_since_peak as f64 >= params.fade_after_s * sample_rate { params.fade_db_per_s } else { 0.0 };
        let (plate_fade, follower_fall) = kept(&mut self.fades, [fade_db_per_s, f64::from(held.size), params.fade_db_per_s], || (fade_gain(fade_db_per_s, f64::from(self.loop_base) * f64::from(held.size) / (PLATE_DECAY_STAGES * sample_rate)), fade_gain(params.fade_db_per_s, 1.0 / sample_rate)));
        if self.tank == Tank::Hall {
            let freezing = held.freeze > f64::from(FREEZE_EPS) || target.freeze > 0.0;
            let mut latched = self.held_frozen_size;
            let t = held.tank(freezing, &mut latched, dispersion, plate_fade, self.loop_base, self.decay_rise);
            self.lattice.prepare(f64::from(held.seconds), f64::from(held.low_times), f64::from(held.high_times), t.diffusion, t.size, t.hold, held.disperse, fade_db_per_s, held.even, sample_rate);
        }
        let mut phase = self.phase;
        let mut tickle = self.tickle;
        let (mut duck_env, mut duck_active) = (self.duck_env, self.duck_active);
        let mut shimmered = self.shimmered;
        let mut blooming = self.blooming;
        let mut spreading = self.spreading;
        let (mut followed, mut since_peak) = (self.followed, self.since_peak);
        let mut blew_up = false;
        let mut settled = None;
        for i in 0..l.len() {
            let fresh;
            let t: &Loop = match &settled {
                Some(t) => t,
                None => {
                    let before = if i == 0 { Some(z) } else { None };
                    z.toward(&target, a);
                    if target.freeze == 1.0 && z.freeze > FREEZE_SNAP {
                        z.freeze = 1.0;
                    }
                    z.settle(&target);
                    let t = z.tank(z.freeze > f64::from(FREEZE_EPS) || target.freeze > 0.0, &mut frozen_size, dispersion, plate_fade, self.loop_base, self.decay_rise);
                    if before == Some(z) {
                        settled.insert(t)
                    } else {
                        fresh = t;
                        &fresh
                    }
                }
            };
            let shimmering = z.shimmer > FREEZE_EPS || target.shimmer > 0.0;
            let taps = pattern(params.early_pattern);
            let how = if z.full { Read::Full } else { Read::Straight };

            let (dry_l, dry_r) = (l[i], r[i]);
            let level = dry_l.abs().max(dry_r.abs());
            if level > 0.0 && level >= followed {
                followed = level;
                since_peak = 0;
            } else {
                followed *= follower_fall;
                since_peak = since_peak.saturating_add(1);
            }
            self.mid.pre.push(0.5 * (dry_l + dry_r));
            let carpet = if plateauing { Plateau::carpet(z.plateau, plateau_fade, z.seconds, sample_rate as f32) } else { 0.0 };
            if plateauing {
                self.plateau.settle();
            }
            let base = f64::from(z.pre_delay) + 1.0;
            let turning = plateauing && drifting;
            if turning {
                self.plateau.turn();
                self.mid.turned.push(0.5 * (dry_l + dry_r));
            }
            let delayed = match (plateauing, turning || z.full) {
                (true, false) => self.plateau.tick(&self.mid.pre, base, f64::from(z.plateau), carpet),
                (true, true) => self.plateau.turned(0, &self.mid.pre, &mut self.mid.turned, base, f64::from(z.plateau), carpet, turning, how),
                (false, _) => self.mid.delayed(base, how),
            };
            let spread = z.stereo > 0.0 || target.stereo > 0.0;
            let delayed_side = if spread {
                spreading = true;
                self.side.pre.push(0.5 * (dry_l - dry_r));
                if turning {
                    self.side.turned.push(0.5 * (dry_l - dry_r));
                }
                match (plateauing, turning || z.full) {
                    (true, false) => self.plateau.tick(&self.side.pre, base, f64::from(z.plateau), carpet),
                    (true, true) => self.plateau.turned(1, &self.side.pre, &mut self.side.turned, base, f64::from(z.plateau), carpet, turning, how),
                    (false, _) => self.side.delayed(base, how),
                }
            } else {
                if spreading {
                    self.side.clear();
                    spreading = false;
                }
                0.0
            };
            let reflections = (z.early > 0.0 || target.early > 0.0).then(|| {
                let (span, gain) = (f64::from(z.span), z.early * t.gate);
                let [lm, rm, ls, rs] = &mut self.early_readers;
                let (mut early_l, mut early_r) = (gain * early(&self.mid.pre, base, span, taps[0], lm, how), gain * early(&self.mid.pre, base, span, taps[1], rm, how));
                if spread {
                    let (side_l, side_r) = (early(&self.side.pre, base, span, taps[0], ls, how), early(&self.side.pre, base, span, taps[1], rs, how));
                    early_l += z.stereo * gain * side_l;
                    early_r -= z.stereo * gain * side_r;
                }
                if z.early_width == 1.0 {
                    (early_l, early_r)
                } else {
                    let (centre, edge) = (0.5 * (early_l + early_r), 0.5 * (early_l - early_r));
                    (centre + z.early_width * edge, centre - z.early_width * edge)
                }
            });
            let early_crossing = z.early_cross > 0.0 || target.early_cross > 0.0;
            let reflections = match reflections {
                Some(early) if early_crossing => Some(self.early_far.crossed(early, f64::from(z.cross_at), z.early_cross, how)),
                uncrossed => {
                    self.early_far.rest();
                    uncrossed
                }
            };
            let reflections = match reflections {
                Some(early) => Some(self.early_scatter.tick(early, z.scatter)),
                None => {
                    self.early_scatter.rest();
                    None
                }
            };
            let placed = tapping.then(|| {
                z.taps.iter().zip(&mut self.tap_readers).fold((0.0, 0.0), |(l, r), (tap, [mid_reader, side_reader])| {
                    let at = f64::from(tap.delay) + 1.0;
                    let mid = mid_reader.read(&self.mid.pre, at, how);
                    let side = if spread { z.stereo * side_reader.read(&self.side.pre, at, how) } else { 0.0 };
                    let (to_l, to_r) = tap.placed(tap.level * t.gate, mid, side);
                    (l + to_l, r + to_r)
                })
            });
            let placed = match placed {
                Some(taps) if early_crossing => Some(self.tap_far.crossed(taps, f64::from(z.cross_at), z.early_cross, how)),
                uncrossed => {
                    self.tap_far.rest();
                    uncrossed
                }
            };
            let placed = match placed {
                Some(taps) => Some(self.tap_scatter.tick(taps, z.tap_scatter)),
                None => {
                    self.tap_scatter.rest();
                    None
                }
            };
            let (bloomed, bloomed_side) = if z.attack > 0.0 {
                blooming = true;
                let gain = ATTACK_GAIN - ATTACK_SHAPE_REACH * z.shape;
                (self.mid.bloomed(delayed, z.attack, gain, z.full), if spread { self.side.bloomed(delayed_side, z.attack, gain, z.full) } else { 0.0 })
            } else {
                if blooming {
                    self.mid.clear_bloom();
                    self.side.clear_bloom();
                    blooming = false;
                }
                (delayed, delayed_side)
            };
            tickle = -tickle;
            let diffused = self.mid.diffused(bloomed, self.dc_pole, tickle * TICKLE, z.spread);
            let side = if spread { z.stereo * self.side.diffused(bloomed_side, self.dc_pole, tickle * TICKLE, z.spread) } else { 0.0 };
            let (diffused, side) = match params.age {
                Age::Now => (diffused, side),
                Age::Eighties => {
                    self.age_mid = one_pole_lp(diffused, self.age_poles[0], self.age_mid);
                    self.age_side = one_pole_lp(side, self.age_poles[0], self.age_side);
                    (self.age_mid, self.age_side)
                }
                Age::Seventies => {
                    self.age_mid = one_pole_lp(diffused, self.age_poles[1], self.age_mid);
                    self.age_side = one_pole_lp(side, self.age_poles[1], self.age_side);
                    if self.age_count == 0 {
                        self.age_hold = (self.age_mid, self.age_side);
                    }
                    self.age_count = (self.age_count + 1) % self.age_every;
                    self.age_out = (one_pole_lp(self.age_hold.0, self.age_poles[1], self.age_out.0), one_pole_lp(self.age_hold.1, self.age_poles[1], self.age_out.1));
                    self.age_out
                }
            };

            let (lfo_a, lfo_b) = if t.depth == 0.0 { (0.0, 0.0) } else { ((2.0 * PI * phase) as f32).sin_cos() };
            phase = wrap(phase + step);
            let wandering = shimmering.then_some((SHIMMER_WANDER_MS * sample_rate / 1000.0) as f32 * z.shimmer * t.gate);

            let far = if z.cross_feed > 0.0 || target.cross_feed > 0.0 {
                let (far_mid, far_side) = self.tank_far.tick((diffused, side), f64::from(z.cross_at), how);
                Some((z.cross_feed * far_mid, z.cross_feed * far_side))
            } else {
                self.tank_far.rest();
                None
            };
            let (mut in_l, mut in_r) = match self.tank {
                Tank::Plate => {
                    let (mut inject_a, mut inject_b) = (0.0, 0.0);
                    if shimmering {
                        let pitched_a = self.a.shimmer.tick(self.a.out, z.ratio, self.loop_low, self.loop_high);
                        let pitched_b = self.b.shimmer.tick(self.b.out, z.ratio, self.loop_low, self.loop_high);
                        inject_a = (z.shimmer * pitched_b).tanh();
                        inject_b = (z.shimmer * pitched_a).tanh();
                    } else if shimmered {
                        self.a.shimmer.reset();
                        self.b.shimmer.reset();
                    }
                    let in_a = diffused * t.gate + self.b.feed(t) + inject_a * t.gate - side * t.gate;
                    let in_b = diffused * t.gate + self.a.feed(t) + inject_b * t.gate + side * t.gate;
                    let (fresh_a, fresh_b) = (diffused * t.gate - side * t.gate, diffused * t.gate + side * t.gate);
                    let (in_a, in_b, fresh_a, fresh_b) = match far {
                        Some((far_mid, far_side)) => {
                            let (far_a, far_b) = (t.gate * (far_mid + far_side), t.gate * (far_mid - far_side));
                            (in_a + far_a, in_b + far_b, fresh_a + far_a, fresh_b + far_b)
                        }
                        None => (in_a, in_b, fresh_a, fresh_b),
                    };
                    let (same_a, cross_a) = self.a.tick(in_a, fresh_a, lfo_a, z.random, wandering, t);
                    let (same_b, cross_b) = self.b.tick(in_b, fresh_b, lfo_b, z.random, wandering, t);
                    self.front_scatter.rest();
                    (0.6 * (same_b - cross_a), 0.6 * (same_a - cross_b))
                }
                Tank::Hall => {
                    let mut inject = (0.0, 0.0);
                    if shimmering {
                        let pitched_l = self.a.shimmer.tick(self.lattice.out.0, z.ratio, self.loop_low, self.loop_high);
                        let pitched_r = self.b.shimmer.tick(self.lattice.out.1, z.ratio, self.loop_low, self.loop_high);
                        inject = ((z.shimmer * pitched_r).tanh(), (z.shimmer * pitched_l).tanh());
                    } else if shimmered {
                        self.a.shimmer.reset();
                        self.b.shimmer.reset();
                    }
                    let fronted = (t.gate * (diffused + side), t.gate * (diffused - side));
                    let fronted = match far {
                        Some((far_mid, far_side)) => (fronted.0 + t.gate * (far_mid - far_side), fronted.1 + t.gate * (far_mid + far_side)),
                        None => fronted,
                    };
                    let front = self.front_scatter.tick(fronted, z.scatter);
                    self.lattice.tick(diffused, side, front, z.front, inject, far, (lfo_a, lfo_b), z.random, wandering, t)
                }
            };
            shimmered = shimmering;
            if params.age != Age::Now {
                in_l = stepped(in_l, AGE_STEPS);
                in_r = stepped(in_r, AGE_STEPS);
            }
            let (mut own_l, mut own_r) = (0.0, 0.0);
            if let Some((early_l, early_r)) = reflections {
                if params.early_own_level {
                    (own_l, own_r) = widen(self.early_left.tick(early_l, &z), self.early_right.tick(early_r, &z), z.width);
                } else {
                    in_l += early_l;
                    in_r += early_r;
                }
            }
            if let Some((tap_l, tap_r)) = placed {
                own_l += self.tap_left.tick(tap_l, &z);
                own_r += self.tap_right.tick(tap_r, &z);
            }
            let (wet_l, wet_r) = widen(self.left.tick(in_l, &z), self.right.tick(in_r, &z), z.width);
            blew_up |= !wet_l.is_finite() || !wet_r.is_finite() || !own_l.is_finite() || !own_r.is_finite();
            let dry = 1.0 - z.mix + z.keep * z.mix;
            duck_env = dsp::follow(duck_env, dry_l.abs().max(dry_r.abs()), detect_attack, detect_release);
            duck_active = dsp::follow(duck_active, f32::from(u8::from(duck_env > threshold)), duck_attack, duck_release);
            let shut = if z.ducking >= 0.0 { duck_active } else { 1.0 - duck_active };
            let duck = 1.0 - z.ducking * z.ducking * shut;
            l[i] = dsp::finite(dry * dry_l + duck * (z.mix * z.forward * wet_l) + duck * (z.mix * own_l));
            r[i] = dsp::finite(dry * dry_r + duck * (z.mix * z.forward * wet_r) + duck * (z.mix * own_r));
        }
        self.controls = Some(z);
        self.frozen_size = frozen_size;
        self.phase = phase;
        self.tickle = tickle;
        self.duck_env = duck_env;
        self.duck_active = duck_active;
        self.shimmered = shimmered;
        self.blooming = blooming;
        self.spreading = spreading;
        self.followed = followed;
        self.since_peak = since_peak;
        self.frames += l.len() as i64;
        if blew_up {
            self.clear();
        }
    }
}

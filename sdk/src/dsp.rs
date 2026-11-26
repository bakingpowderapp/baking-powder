pub mod crossover;
pub mod ears;
pub mod envelope;
pub mod fft;
pub mod filter;
pub mod ladder;
pub mod noise;
pub mod svf;
pub mod unison;

use std::f64::consts::{LN_2, PI};
use std::ops::{Add, Mul, Sub};

const MIX_SECONDS: f64 = 0.010;
const MIX_SNAP: f32 = 1e-5;
const FEEDBACK_SHAPE: f64 = 1.6;
const FEEDBACK_MAX: f64 = 0.97;
pub const WIDTH_UNITY_EPS: f32 = 0.0001;
pub const SINC_HALF: usize = 32;
const SINC_EPSILON: f64 = 1e-9;
const NEWEST: usize = 1;

pub struct Ring {
    buf: Vec<f32>,
    mask: usize,
    w: usize,
}

impl Ring {
    pub fn new(samples: usize) -> Ring {
        let len = samples.max(2).next_power_of_two();
        Ring { buf: vec![0.0; len], mask: len - 1, w: 0 }
    }

    pub fn clear(&mut self) {
        self.buf.fill(0.0);
    }

    pub fn push(&mut self, x: f32) {
        self.buf[self.w] = x;
        self.w = (self.w + 1) & self.mask;
    }

    pub fn tap(&self, delay: usize) -> f32 {
        self.buf[self.w.wrapping_sub(delay) & self.mask]
    }

    pub fn linear(&self, delay: f64) -> f32 {
        let delay = delay.max(NEWEST as f64);
        let whole = delay.floor();
        let frac = (delay - whole) as f32;
        let i = whole as usize;
        let a = self.tap(i);
        let b = self.tap(i + 1);
        a + frac * (b - a)
    }

    pub fn hermite(&self, delay: f64) -> f32 {
        let delay = delay.max(NEWEST as f64);
        let whole = delay.floor();
        let frac = (delay - whole) as f32;
        let i = whole as usize;
        let (xm1, x0, x1, x2) = (self.tap((i - 1).max(NEWEST)), self.tap(i), self.tap(i + 1), self.tap(i + 2));
        let c1 = 0.5 * (x1 - xm1);
        let c2 = xm1 - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
        let c3 = 0.5 * (x2 - xm1) + 1.5 * (x0 - x1);
        ((c3 * frac + c2) * frac + c1) * frac + x0
    }

    pub fn bandlimited(&self, delay: f64, cutoff: f64) -> f32 {
        let delay = delay.max((NEWEST + SINC_HALF - 1) as f64);
        let whole = delay.floor();
        let first = SINC_HALF as f64 - (delay - whole);
        let oldest = whole as usize + SINC_HALF;
        let (mut sin, mut cos) = (PI * cutoff * first).sin_cos();
        let (step_sin, step_cos) = (PI * cutoff).sin_cos();
        let (mut window_sin, mut window_cos) = (PI * first / SINC_HALF as f64).sin_cos();
        let (window_step_sin, window_step_cos) = (PI / SINC_HALF as f64).sin_cos();
        let mut t = first;
        let mut sum = 0.0;
        let mut weight = 0.0;
        for k in 0..2 * SINC_HALF {
            let sinc = if t.abs() < SINC_EPSILON { cutoff } else { sin / (PI * t) };
            let h = sinc * (0.5 + 0.5 * window_cos);
            sum += f64::from(self.tap(oldest - k)) * h;
            weight += h;
            (sin, cos) = (sin * step_cos - cos * step_sin, cos * step_cos + sin * step_sin);
            (window_sin, window_cos) = (window_sin * window_step_cos - window_cos * window_step_sin, window_cos * window_step_cos + window_sin * window_step_sin);
            t -= 1.0;
        }
        if weight.abs() < SINC_EPSILON { 0.0 } else { (sum / weight) as f32 }
    }

    pub fn allpass(&mut self, delay: usize, g: f32, x: f32) -> f32 {
        let r = self.tap(delay);
        let v = x - g * r;
        self.push(v);
        g * v + r
    }

    pub fn allpass_linear(&mut self, delay: f64, g: f32, x: f32) -> f32 {
        let r = self.linear(delay);
        let v = x - g * r;
        self.push(v);
        g * v + r
    }
}

pub struct Greatest {
    ring: Vec<(f64, u64)>,
    head: usize,
    len: usize,
    now: u64,
}

impl Greatest {
    pub fn new(room: usize) -> Greatest {
        Greatest { ring: vec![(0.0, 0); room.max(1)], head: 0, len: 0, now: 0 }
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    pub fn greatest(&mut self, x: f64, window: u64) -> f64 {
        let room = self.ring.len();
        self.now += 1;
        while self.len > 0 && self.ring[(self.head + self.len - 1) % room].0 <= x {
            self.len -= 1;
        }
        self.ring[(self.head + self.len) % room] = (x, self.now);
        self.len += 1;
        while self.len > 1 && self.ring[self.head].1 + window.max(1) <= self.now {
            self.head = (self.head + 1) % room;
            self.len -= 1;
        }
        self.ring[self.head].0
    }
}

pub fn smoothing(seconds: f64, sample_rate: f64) -> f64 {
    1.0 - (-1.0 / (seconds * sample_rate)).exp()
}

#[derive(Clone, Copy, Debug)]
pub struct Smooth {
    pub z: f64,
    coeff: f64,
}

impl Smooth {
    pub fn new(initial: f64, seconds: f64, sample_rate: f64) -> Smooth {
        Smooth { z: initial, coeff: smoothing(seconds, sample_rate) }
    }

    pub fn snap(&mut self, value: f64) {
        self.z = value;
    }

    pub fn tick(&mut self, target: f64) -> f64 {
        self.z += self.coeff * (target - self.z);
        self.z
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Mix {
    pub z: f32,
    coeff: f32,
    fresh: bool,
}

impl Mix {
    pub fn new(sample_rate: f64) -> Mix {
        Mix { z: 0.0, coeff: smoothing(MIX_SECONDS, sample_rate) as f32, fresh: true }
    }

    pub fn tick(&mut self, wet: f32) -> f32 {
        if self.fresh {
            self.fresh = false;
            self.z = wet;
            return wet;
        }
        self.z += self.coeff * (wet - self.z);
        if (wet - self.z).abs() < MIX_SNAP {
            self.z = wet;
        }
        self.z
    }
}

pub fn one_pole_lp(x: f32, coeff: f32, prev: f32) -> f32 {
    (1.0 - coeff) * x + coeff * prev
}

pub fn settle(x: f32, target: f32, a: f32) -> f32 {
    let next = x + a * (target - x);
    if next == x { target } else { next }
}

pub fn follow(z: f32, x: f32, rise: f32, fall: f32) -> f32 {
    let a = if x > z { rise } else { fall };
    z + a * (x - z)
}

#[derive(Clone, Copy, Debug, Default)]
pub struct OnePoleHp<T = f32> {
    x1: T,
    y1: T,
}

impl<T: Copy + Add<Output = T> + Sub<Output = T> + Mul<Output = T>> OnePoleHp<T> {
    pub fn tick(&mut self, x: T, coeff: T) -> T {
        let y = coeff * (self.y1 + x - self.x1);
        self.x1 = x;
        self.y1 = y;
        y
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DcBlock {
    x1: f32,
    y1: f32,
}

impl DcBlock {
    pub fn prime(&mut self, x: f32, y: f32) {
        self.x1 = x;
        self.y1 = y;
    }

    pub fn tick(&mut self, x: f32, pole: f32) -> f32 {
        let y = x - self.x1 + pole * self.y1;
        self.x1 = x;
        self.y1 = y;
        y
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DcCanonical {
    v: f32,
}

impl DcCanonical {
    pub fn prime(&mut self, x: f32, pole: f32) {
        self.v = x / (1.0 - pole);
    }

    pub fn tick(&mut self, x: f32, pole: f32) -> f32 {
        let v = x + pole * self.v;
        let y = v - self.v;
        self.v = v;
        y
    }
}

pub fn lp_coeff(hz: f64, sample_rate: f64) -> f32 {
    (-2.0 * PI * hz / sample_rate).exp() as f32
}

pub fn db_to_gain(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

pub fn finite(x: f32) -> f32 {
    if x.is_finite() { x } else { 0.0 }
}

pub fn sanitize(l: &mut [f32], r: &mut [f32]) {
    for x in l.iter_mut().chain(r.iter_mut()) {
        *x = finite(*x);
    }
}

pub fn sine(phase: f64) -> f64 {
    (2.0 * PI * (phase - phase.floor())).sin()
}

pub fn triangle(phase: f64) -> f64 {
    let p = phase - phase.floor();
    4.0 * (p - 0.5).abs() - 1.0
}

pub fn square(phase: f64) -> f64 {
    if phase - phase.floor() < 0.5 { 1.0 } else { -1.0 }
}

pub fn wrap(phase: f64) -> f64 {
    if phase >= 1.0 { phase - 1.0 } else { phase }
}

pub fn widen(l: f32, r: f32, width: f32) -> (f32, f32) {
    let mid = 0.5 * (l + r);
    let side = 0.5 * (l - r) * width;
    (mid + side, mid - side)
}

pub fn shaped_feedback(feedback: f64) -> f32 {
    let shaped = 1.0 - (1.0 - feedback.abs()).powf(FEEDBACK_SHAPE);
    let gain = if feedback >= 0.0 { FEEDBACK_MAX } else { -FEEDBACK_MAX };
    (gain * shaped) as f32
}

pub fn sample_hold(cycle: i64, seed: u64) -> f64 {
    let mut z = (cycle as u64).wrapping_add(seed).wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 * (2.0 / 9_007_199_254_740_992.0) - 1.0
}

pub fn algebraic(x: f64) -> f64 {
    x / (1.0 + x * x).sqrt()
}

pub fn algebraic_integral(x: f64) -> f64 {
    let r = (1.0 + x * x).sqrt();
    x * x / (r + 1.0)
}

pub fn tanh_integral(x: f64) -> f64 {
    let a = x.abs();
    a + (-2.0 * a).exp().ln_1p() - LN_2
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Halfband {
    a0: (f32, f32),
    a1: (f32, f32),
    b0: (f32, f32),
    b1: (f32, f32),
}

const HALFBAND_A0: f32 = 0.07471;
const HALFBAND_A1: f32 = 0.63036;
const HALFBAND_B0: f32 = 0.32424;
const HALFBAND_B1: f32 = 0.85945;

fn allpass_section(x: f32, a: f32, state: &mut (f32, f32)) -> f32 {
    let y = a * (x - state.1) + state.0;
    *state = (x, y);
    y
}

impl Halfband {
    pub fn upsample(&mut self, x: f32) -> (f32, f32) {
        let a = allpass_section(allpass_section(x, HALFBAND_A0, &mut self.a0), HALFBAND_A1, &mut self.a1);
        let b = allpass_section(allpass_section(x, HALFBAND_B0, &mut self.b0), HALFBAND_B1, &mut self.b1);
        (a, b)
    }

    pub fn downsample(&mut self, x0: f32, x1: f32) -> f32 {
        let a = allpass_section(allpass_section(x1, HALFBAND_A0, &mut self.a0), HALFBAND_A1, &mut self.a1);
        let b = allpass_section(allpass_section(x0, HALFBAND_B0, &mut self.b0), HALFBAND_B1, &mut self.b1);
        0.5 * (a + b)
    }

    pub fn series(&mut self, x: f32) -> f32 {
        let a = allpass_section(allpass_section(x, HALFBAND_A0, &mut self.a0), HALFBAND_A1, &mut self.a1);
        allpass_section(allpass_section(a, HALFBAND_B0, &mut self.b0), HALFBAND_B1, &mut self.b1)
    }

    pub fn reset(&mut self) {
        *self = Halfband::default();
    }

    pub fn settle(&mut self, x: f32) {
        *self = Halfband { a0: (x, x), a1: (x, x), b0: (x, x), b1: (x, x) };
    }
}

pub struct PitchShifter {
    ring: Ring,
    grain: f32,
    jitter: f32,
    guard: f32,
    phase: f32,
    seed: u32,
    rng: u32,
    jitters: [f32; 4],
    last: [f32; 4],
}

impl PitchShifter {
    pub fn new(sample_rate: f64, grain_ms: f64, jitter_ms: f64, seed: u32) -> PitchShifter {
        let grain = (grain_ms * 0.001 * sample_rate).max(64.0) as f32;
        let jitter = (jitter_ms.max(0.0) * 0.001 * sample_rate) as f32;
        let seed = if seed == 0 { 0x9E37_79B9 } else { seed };
        PitchShifter {
            ring: Ring::new((2.0 * grain + jitter + 8.0) as usize),
            grain,
            jitter,
            guard: 4.0 + jitter,
            phase: 0.0,
            seed,
            rng: seed,
            jitters: [0.0; 4],
            last: [0.0; 4],
        }
    }

    pub fn reset(&mut self) {
        self.ring.clear();
        self.phase = 0.0;
        self.rng = self.seed;
        self.jitters = [0.0; 4];
        self.last = [0.0; 4];
    }

    pub fn process(&mut self, input: f32, ratio: f32) -> f32 {
        self.ring.push(input);
        let up = ratio >= 1.0;
        let mut acc = 0.0;
        for tap in 0..4 {
            let phi = self.phase + tap as f32 * 0.25;
            let p = phi - phi.floor();
            if self.jitter > 0.0 && p < self.last[tap] {
                self.jitters[tap] = crate::dsp::noise::white(&mut self.rng) as f32 * self.jitter;
            }
            self.last[tap] = p;
            let sweep = if up { self.grain - p * self.grain } else { p * self.grain };
            let delay = self.guard + sweep + self.jitters[tap];
            let window = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * p).cos();
            acc += self.ring.hermite(f64::from(delay) + 1.0) * window;
        }
        self.phase += (1.0 - ratio).abs() / self.grain;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        acc * 0.5
    }
}

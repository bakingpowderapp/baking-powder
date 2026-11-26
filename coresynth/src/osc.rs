use crate::minblep::{MIN_BLEP_LEN, min_blep};
use crate::params::Wave;
use sdk::face::Drawn;
use std::f64::consts::PI;

const HALF_SINE_MEAN: f64 = 1.0 / PI;
const FULL_SINE_MEAN: f64 = 2.0 / PI;
const STAIR_STEP: f64 = 2.0 / 3.0;
const TRI_SAW_PEAK: f64 = 0.25;
const TRI_SAW_RISE: f64 = 8.0;
const TRI_SAW_FALL: f64 = 8.0 / 3.0;
const SOFT_DRIVE: f64 = 4.0;
const SOFT_NORM: f64 = 1.000_671_150_401_682_5;
const ORGAN_HARMONICS: usize = 4;
const ORGAN_PEAK: f64 = 1.527_278_662_278_219_5;
const CYCLE_SAMPLES: usize = 2048;
const CYCLE_STEP: f64 = 1.0 / CYCLE_SAMPLES as f64;
const LEAST_SMOOTHED_STEP: f64 = 1e-6;

pub fn poly_blep(t: f64, dt: f64) -> f64 {
    if t < 0.0 {
        0.0
    } else if t < dt {
        let x = t / dt;
        2.0 * x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + 2.0 * x + 1.0
    } else {
        0.0
    }
}

pub fn saw(phase: f64, dt: f64) -> f64 {
    (2.0 * phase - 1.0) - poly_blep(phase, dt)
}

pub fn square(phase: f64, dt: f64, pulse_width: f64) -> f64 {
    let mut y = if phase < pulse_width { 1.0 } else { -1.0 };
    y += poly_blep(phase, dt);
    let mut t = phase - pulse_width;
    if t < 0.0 {
        t += 1.0;
    }
    y - poly_blep(t, dt)
}

pub fn pulse(phase: f64, dt: f64, pulse_width: f64) -> f64 {
    square(phase, dt, pulse_width) - (2.0 * pulse_width - 1.0)
}

fn centred(phase: f64) -> f64 {
    if phase < 0.5 { phase - 0.25 } else { 0.75 - phase }
}

fn blamp(t: f64, dt: f64) -> f64 {
    if t < 0.0 {
        0.0
    } else if t < dt {
        let d = 1.0 - t / dt;
        d * d * d / 3.0
    } else if t > 1.0 - dt {
        let d = (t - 1.0) / dt + 1.0;
        d * d * d / 3.0
    } else {
        0.0
    }
}

pub fn triangle(phase: f64, dt: f64) -> f64 {
    let fall = phase - 0.5;
    let fall = if fall < 0.0 { fall + 1.0 } else { fall };
    4.0 * (centred(phase) + dt * (blamp(phase, dt) - blamp(fall, dt)))
}

pub fn sine(phase: f64) -> f64 {
    (2.0 * PI * phase).sin()
}

pub fn naive_saw(phase: f64) -> f64 {
    2.0 * phase - 1.0
}

pub fn naive_pulse(phase: f64, pulse_width: f64) -> f64 {
    (if phase < pulse_width { 1.0 } else { -1.0 }) - (2.0 * pulse_width - 1.0)
}

fn naive_square(phase: f64, pulse_width: f64) -> f64 {
    if phase < pulse_width { 1.0 } else { -1.0 }
}

fn naive_triangle(phase: f64) -> f64 {
    1.0 - 2.0 * (2.0 * phase - 1.0).abs()
}

fn naive_double_saw(phase: f64) -> f64 {
    4.0 * (phase - 0.5 * (2.0 * phase).floor()) - 1.0
}

fn naive_stair(phase: f64) -> f64 {
    STAIR_STEP * (4.0 * phase).floor() - 1.0
}

fn naive_tri_saw(phase: f64) -> f64 {
    if phase < TRI_SAW_PEAK { TRI_SAW_RISE * phase - 1.0 } else { 1.0 - TRI_SAW_FALL * (phase - TRI_SAW_PEAK) }
}

fn soft(phase: f64) -> f64 {
    (SOFT_DRIVE * sine(phase)).tanh() * SOFT_NORM
}

fn organ(phase: f64) -> f64 {
    (1..=ORGAN_HARMONICS).map(|harmonic| (2.0 * PI * harmonic as f64 * phase).sin() / harmonic as f64).sum::<f64>() / ORGAN_PEAK
}

fn sine_cubed(phase: f64) -> f64 {
    let s = sine(phase);
    s * s * s
}

fn turned(phase: f64, by: f64) -> f64 {
    let turned = phase - by;
    if turned < 0.0 { turned + 1.0 } else { turned }
}

#[derive(Clone, Debug)]
pub struct Cycle {
    heights: Vec<f64>,
    areas: Vec<f64>,
    volumes: Vec<f64>,
}

impl Cycle {
    pub fn flat() -> Cycle {
        Cycle { heights: vec![0.0; CYCLE_SAMPLES + 1], areas: vec![0.0; CYCLE_SAMPLES + 1], volumes: vec![0.0; CYCLE_SAMPLES + 1] }
    }

    pub fn of(drawn: &Drawn) -> Cycle {
        let mut cycle = Cycle::flat();
        cycle.trace(drawn);
        cycle
    }

    pub fn trace(&mut self, drawn: &Drawn) {
        for (index, height) in self.heights[..CYCLE_SAMPLES].iter_mut().enumerate() {
            *height = 2.0 * drawn.value_at(index as f64 * CYCLE_STEP) - 1.0;
        }
        let mean = self.heights[..CYCLE_SAMPLES].iter().sum::<f64>() * CYCLE_STEP;
        for height in &mut self.heights[..CYCLE_SAMPLES] {
            *height -= mean;
        }
        self.heights[CYCLE_SAMPLES] = self.heights[0];
        self.areas[0] = 0.0;
        for index in 0..CYCLE_SAMPLES {
            self.areas[index + 1] = self.areas[index] + CYCLE_STEP * 0.5 * (self.heights[index] + self.heights[index + 1]);
        }
        let mean: f64 = (0..CYCLE_SAMPLES).map(|index| self.swept(index, CYCLE_STEP)).sum();
        for area in &mut self.areas {
            *area -= mean;
        }
        self.volumes[0] = 0.0;
        for index in 0..CYCLE_SAMPLES {
            self.volumes[index + 1] = self.volumes[index] + self.swept(index, CYCLE_STEP);
        }
    }

    fn swept(&self, index: usize, along: f64) -> f64 {
        let (from, to) = (self.heights[index], self.heights[index + 1]);
        along * (self.areas[index] + along * (from / 2.0 + along * (to - from) / (6.0 * CYCLE_STEP)))
    }

    fn volume(&self, phase: f64) -> f64 {
        let (index, along) = cell(phase);
        self.volumes[index] + self.swept(index, along)
    }

    pub fn value(&self, phase: f64) -> f64 {
        let (index, along) = cell(phase);
        self.heights[index] + (self.heights[index + 1] - self.heights[index]) * along / CYCLE_STEP
    }

    pub fn smoothed(&self, phase: f64, dt: f64) -> f64 {
        if dt < LEAST_SMOOTHED_STEP {
            return self.value(phase);
        }
        (self.volume(turned(phase, dt)) - 2.0 * self.volume(phase) + self.volume(turned(phase, 1.0 - dt))) / (dt * dt)
    }
}

fn cell(phase: f64) -> (usize, f64) {
    let phase = if phase < 0.0 { phase - phase.floor() } else { phase };
    let index = ((phase * CYCLE_SAMPLES as f64) as usize).min(CYCLE_SAMPLES - 1);
    (index, phase - index as f64 * CYCLE_STEP)
}

pub fn naive_wave(wave: Wave, phase: f64, pulse_width: f64, cycle: &Cycle) -> f64 {
    match wave {
        Wave::Saw => naive_saw(phase),
        Wave::Square => naive_square(phase, pulse_width),
        Wave::Triangle => naive_triangle(phase),
        Wave::Sine => sine(phase),
        Wave::HalfSine => 2.0 * (if phase < 0.5 { (2.0 * PI * phase).sin() } else { 0.0 } - HALF_SINE_MEAN),
        Wave::FullSine => 2.0 * ((2.0 * PI * phase).sin().abs() - FULL_SINE_MEAN),
        Wave::SinePulse => {
            if phase < 0.5 {
                (4.0 * PI * phase).sin()
            } else {
                0.0
            }
        }
        Wave::Parabola => {
            let x = 2.0 * phase - 1.0;
            2.0 / 3.0 - 2.0 * x * x
        }
        Wave::Trapezoid => {
            if phase < 0.25 {
                8.0 * phase - 1.0
            } else if phase < 0.5 {
                1.0
            } else if phase < 0.75 {
                5.0 - 8.0 * phase
            } else {
                -1.0
            }
        }
        Wave::DoubleSaw => naive_double_saw(phase),
        Wave::Stair => naive_stair(phase),
        Wave::TriSaw => naive_tri_saw(phase),
        Wave::Soft => soft(phase),
        Wave::Organ => organ(phase),
        Wave::SineCubed => sine_cubed(phase),
        Wave::Drawn => cycle.value(phase),
    }
}

fn double_saw(phase: f64, dt: f64) -> f64 {
    naive_double_saw(phase) - poly_blep(phase, dt) - poly_blep(turned(phase, 0.5), dt)
}

fn stair(phase: f64, dt: f64) -> f64 {
    naive_stair(phase) + (poly_blep(turned(phase, 0.25), dt) + poly_blep(turned(phase, 0.5), dt) + poly_blep(turned(phase, 0.75), dt)) / 3.0 - poly_blep(phase, dt)
}

fn kinked(wave: Wave, phase: f64, dt: f64, cycle: &Cycle) -> f64 {
    let half = turned(phase, 0.5);
    let kinks = match wave {
        Wave::HalfSine => 2.0 * PI * (blamp(phase, dt) + blamp(half, dt)),
        Wave::FullSine => 4.0 * PI * (blamp(phase, dt) + blamp(half, dt)),
        Wave::SinePulse => 2.0 * PI * (blamp(phase, dt) - blamp(half, dt)),
        Wave::Parabola => 8.0 * blamp(phase, dt),
        Wave::Trapezoid => 4.0 * (blamp(phase, dt) - blamp(turned(phase, 0.25), dt) - blamp(half, dt) + blamp(turned(phase, 0.75), dt)),
        Wave::TriSaw => (TRI_SAW_RISE + TRI_SAW_FALL) / 2.0 * (blamp(phase, dt) - blamp(turned(phase, TRI_SAW_PEAK), dt)),
        _ => 0.0,
    };
    naive_wave(wave, phase, 0.5, cycle) + dt * kinks
}

#[inline(never)]
pub fn shaped(wave: Wave, phase: f64, dt: f64, pulse_width: f64, share: f64, cycle: &Cycle) -> f64 {
    match wave {
        Wave::Saw => blended(share, naive_saw(phase), || saw(phase, dt)),
        Wave::Square => blended(share, naive_pulse(phase, pulse_width), || pulse(phase, dt, pulse_width)),
        Wave::Triangle => triangle(phase, dt),
        Wave::Sine => sine(phase),
        Wave::DoubleSaw => blended(share, naive_double_saw(phase), || double_saw(phase, dt)),
        Wave::Stair => blended(share, naive_stair(phase), || stair(phase, dt)),
        Wave::Soft => soft(phase),
        Wave::Organ => organ(phase),
        Wave::SineCubed => sine_cubed(phase),
        Wave::HalfSine | Wave::FullSine | Wave::SinePulse | Wave::Parabola | Wave::Trapezoid | Wave::TriSaw => kinked(wave, phase, dt, cycle),
        Wave::Drawn => cycle.smoothed(phase, dt),
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Stepped {
    saw: f64,
    square: f64,
    double: f64,
    stair: f64,
}

impl Stepped {
    pub fn of(shapes: &[Wave], levels: &[f64]) -> Stepped {
        let mut stepped = Stepped::default();
        for (wave, level) in shapes.iter().zip(levels) {
            match wave {
                Wave::Saw => stepped.saw += level,
                Wave::Square => stepped.square += level,
                Wave::DoubleSaw => stepped.double += level,
                Wave::Stair => stepped.stair += level,
                _ => {}
            }
        }
        stepped
    }

    pub fn net(&self) -> f64 {
        -2.0 * (self.saw + self.double)
    }

    fn edges(&self, pulse_width: f64) -> [(f64, f64); 5] {
        let stair = STAIR_STEP * self.stair;
        [(pulse_width, -2.0 * self.square), (0.25, stair), (0.5, stair - 2.0 * self.double), (0.75, stair), (1.0, 2.0 * (self.square - self.saw - self.double - self.stair))]
    }

    fn widened(&self, phase: f64, from: f64, to: f64) -> f64 {
        self.square * (naive_pulse(phase, to) - naive_pulse(phase, from))
    }

    pub fn glides(&self, step: f64) -> bool {
        (2.0 * self.square * step).abs() < LEAST_EDGE
    }
}

const MIN_BLEP_OVERSAMPLE: usize = 64;
const MIN_BLEP_TAIL: usize = 32;
const STEP_RING: usize = 64;
const LEAST_EDGE: f64 = 1.0 / 128.0;
const EDGE_BLEND_START: f64 = 0.05;
const EDGE_BLEND_END: f64 = 0.08;

pub struct Kernel {
    table: [f32; MIN_BLEP_LEN],
    rows: [f64; MIN_BLEP_TAIL],
    area: f64,
}

impl Kernel {
    const fn of(table: [f32; MIN_BLEP_LEN]) -> Kernel {
        let mut rows = [0.0; MIN_BLEP_TAIL];
        let mut k = 0;
        while k < MIN_BLEP_TAIL {
            let mut sum = 0.0;
            let mut j = 0;
            while j < MIN_BLEP_OVERSAMPLE {
                sum += table[k * MIN_BLEP_OVERSAMPLE + j] as f64;
                j += 1;
            }
            rows[k] = sum / MIN_BLEP_OVERSAMPLE as f64;
            k += 1;
        }
        let mut area = 0.0;
        let mut i = 0;
        while i < MIN_BLEP_LEN {
            area += table[i] as f64;
            i += 1;
        }
        Kernel { table, rows, area: area / MIN_BLEP_OVERSAMPLE as f64 }
    }
}

const fn box_smoothed(step: [f64; MIN_BLEP_LEN]) -> [f64; MIN_BLEP_LEN] {
    let mut out = [0.0; MIN_BLEP_LEN];
    let mut sum = 0.0;
    let mut i = 0;
    while i < MIN_BLEP_LEN {
        sum += step[i];
        if i >= MIN_BLEP_OVERSAMPLE {
            sum -= step[i - MIN_BLEP_OVERSAMPLE];
        }
        out[i] = sum / MIN_BLEP_OVERSAMPLE as f64;
        i += 1;
    }
    out
}

const fn plain_table(sharp: [f32; MIN_BLEP_LEN]) -> [f32; MIN_BLEP_LEN] {
    let mut step = [0.0; MIN_BLEP_LEN];
    let mut i = 0;
    while i < MIN_BLEP_LEN {
        step[i] = sharp[i] as f64 + 1.0;
        i += 1;
    }
    let smoothed = box_smoothed(box_smoothed(step));
    let mut out = [0.0f32; MIN_BLEP_LEN];
    i = 0;
    while i < MIN_BLEP_LEN {
        out[i] = (smoothed[i] - 1.0) as f32;
        i += 1;
    }
    out
}

static BRIGHT_EDGE: Kernel = Kernel::of(min_blep());
static PLAIN_EDGE: Kernel = Kernel::of(plain_table(min_blep()));

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Edge {
    #[default]
    Plain,
    Bright,
}

impl Edge {
    pub fn of(bright: bool) -> Edge {
        if bright { Edge::Bright } else { Edge::Plain }
    }

    fn kernel(self) -> &'static Kernel {
        match self {
            Edge::Plain => &PLAIN_EDGE,
            Edge::Bright => &BRIGHT_EDGE,
        }
    }

    pub fn steps(self, steps: f64) -> [f64; 2] {
        let mut out = [0.0; 2];
        out[self as usize] = steps;
        out
    }
}

pub fn edge_share(dt: f64) -> f64 {
    if dt <= EDGE_BLEND_START {
        return 1.0;
    }
    ((EDGE_BLEND_END - dt) / (EDGE_BLEND_END - EDGE_BLEND_START)).clamp(0.0, 1.0)
}

pub fn edge_steps(net: f64, share: f64) -> f64 {
    if share > 0.0 { net } else { 0.0 }
}

pub fn blended(share: f64, naive: f64, limited: impl FnOnce() -> f64) -> f64 {
    if share >= 1.0 {
        naive
    } else if share <= 0.0 {
        limited()
    } else {
        share * naive + (1.0 - share) * limited()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct StepRing {
    buf: [f32; STEP_RING],
    head: usize,
    bias: f32,
    rate: [f64; 2],
    rates: [[f64; MIN_BLEP_TAIL]; 2],
    quiet: [usize; 2],
    steady: usize,
}

impl Default for StepRing {
    fn default() -> StepRing {
        StepRing { buf: [0.0; STEP_RING], head: 0, bias: 0.0, rate: [0.0; 2], rates: [[0.0; MIN_BLEP_TAIL]; 2], quiet: [MIN_BLEP_TAIL; 2], steady: MIN_BLEP_TAIL }
    }
}

impl StepRing {
    pub fn pop(&mut self) -> f32 {
        let bias = if self.steady < MIN_BLEP_TAIL { self.settling() } else { self.bias };
        let v = self.buf[self.head] + bias;
        self.buf[self.head] = 0.0;
        self.head = (self.head + 1) % STEP_RING;
        v
    }

    #[inline(never)]
    fn settling(&mut self) -> f32 {
        let slot = self.head % MIN_BLEP_TAIL;
        for edge in [Edge::Bright, Edge::Plain] {
            let rate = self.rate[edge as usize];
            self.rates[edge as usize][slot] = rate;
            let quiet = &mut self.quiet[edge as usize];
            *quiet = if rate == 0.0 { (*quiet + 1).min(MIN_BLEP_TAIL) } else { 0 };
        }
        self.steady += 1;
        let owed: f64 = [Edge::Bright, Edge::Plain].into_iter().filter(|edge| self.quiet[*edge as usize] < MIN_BLEP_TAIL).map(|edge| self.owed(edge, slot)).sum();
        (-owed) as f32
    }

    fn owed(&self, edge: Edge, slot: usize) -> f64 {
        let history = &self.rates[edge as usize];
        edge.kernel().rows.iter().enumerate().map(|(k, row)| row * history[(slot + MIN_BLEP_TAIL - k) % MIN_BLEP_TAIL]).sum()
    }

    #[inline(never)]
    pub fn stamp(&mut self, edge: Edge, amplitude: f32, fractional_offset: f32) {
        let table = &edge.kernel().table;
        let last = MIN_BLEP_LEN - 1;
        let mut position = fractional_offset * MIN_BLEP_OVERSAMPLE as f32;
        for k in 0..MIN_BLEP_TAIL {
            let i0 = position as usize;
            let f = position - i0 as f32;
            let a = table[i0.min(last)];
            let b = table[(i0 + 1).min(last)];
            self.buf[(self.head + k) % STEP_RING] += amplitude * (a + f * (b - a));
            position += MIN_BLEP_OVERSAMPLE as f32;
        }
    }

    pub fn widen(&mut self, edge: Edge, phase: f64, stepped: &Stepped, from: f64, to: f64) {
        let step = stepped.widened(phase, from, to);
        if (phase < from) != (phase < to) || step.abs() >= LEAST_EDGE {
            self.stamp(edge, step as f32, 0.0);
        }
    }

    pub fn clear(&mut self) {
        *self = StepRing::default();
    }

    pub fn settle(&mut self, steps: [f64; 2], dt: f64) {
        let rate = steps.map(|steps| steps * dt);
        if rate != self.rate {
            self.rate = rate;
            self.bias = (-BRIGHT_EDGE.area * steps[Edge::Bright as usize] * dt - PLAIN_EDGE.area * steps[Edge::Plain as usize] * dt) as f32;
            self.steady = 0;
        }
    }

    pub fn edges(&mut self, edge: Edge, phase: f64, dt: f64, from: f64, to: f64, stepped: &Stepped, pulse_width: f64, rise: f64) {
        if dt <= 0.0 {
            return;
        }
        let reach = phase + (to - from) * dt;
        let mut edges = stepped.edges(pulse_width);
        if rise != 0.0 {
            let fall = std::mem::take(&mut edges[0].1);
            let turn = if rise > dt { -fall } else { fall };
            for shift in [0.0, 1.0] {
                let meet = (pulse_width + shift - phase + from * dt) / (dt - rise);
                let at = phase + (meet - from) * dt;
                if turn != 0.0 && phase < at && at <= reach {
                    self.stamp(edge, turn as f32, (1.0 - meet) as f32);
                }
            }
        }
        for shift in [0.0, 1.0] {
            for (at, step) in edges {
                let at = at + shift;
                if step != 0.0 && phase < at && at <= reach {
                    self.stamp(edge, step as f32, (1.0 - from - (at - phase) / dt) as f32);
                }
            }
        }
    }

    pub fn edged(&mut self, edge: Edge, phase: f64, dt: f64, share: f64, stepped: &Stepped, pulse_width: f64, rise: f64) -> f64 {
        self.settle(edge.steps(edge_steps(stepped.net(), share)), dt);
        let due = share * f64::from(self.pop());
        if share > 0.0 {
            self.edges(edge, phase, dt, 0.0, 1.0, stepped, pulse_width, rise);
        }
        due
    }
}

use crate::lfo_shape::{begun, turns};
use crate::{Clock, Unit};
use sdk::dsp::{self, Mix, OnePoleHp};
use crate::params::{self, PhaserShape, Routing, STAGES};
use std::f64::consts::PI;

const MAX_STAGES: usize = 16;
const GLIDE_SECONDS: f64 = 0.030;
const SQUARE_SECONDS: f64 = 0.002;
const FADE_SECONDS: f64 = 0.010;
const FADE_MIN_FRAMES: usize = 64;
const SWEEP_MIN_HZ: f64 = 20.0;
const SWEEP_MAX_HZ: f64 = 18_000.0;
const NYQUIST_GUARD_HZ: f64 = 1000.0;
const VINTAGE_DC_OFFSET: f32 = 0.04;
const TICKLE: f32 = 1e-25;
const SEED_A: u64 = 0x9E37_79B9;
const SEED_B: u64 = 0x85EB_CA77;
const SPREAD_SETTLE: f64 = 1e-6;
const TWIN_SECONDS: f64 = 0.010;
const TWIN_STRIDE: usize = 8;
const TWIN_GROWN: f64 = 1.386;
const TWIN_INSIDE: f32 = 0.9;
const TWIN_REGAIN: f64 = 2.0;
const TWIN_LEAST: f32 = 1e-3;
const TWIN_SPAN: f64 = 1e20;
const EASE_SECONDS: f64 = 0.002;
const CAP_KNEE_DB: f64 = 18.0;
const CAP_TOP_DB: f64 = 24.0;

fn snap_stages(count: i32) -> usize {
    STAGES.iter().copied().min_by_key(|stage| (*stage as i32 - count).abs()).unwrap_or(4)
}

#[derive(Clone, Copy, PartialEq)]
struct Topology {
    routing: Routing,
    stages_a: usize,
    stages_b: usize,
}

impl Topology {
    fn of(params: &params::Phaser) -> Topology {
        Topology { routing: params.routing, stages_a: params.stages, stages_b: snap_stages(params.stages as i32 + params.stage_offset) }
    }
}

#[derive(Clone, Copy)]
struct Knobs {
    rate: f64,
    ratio: f64,
    center: f64,
    range: f64,
    feedback: f32,
    drive: f32,
    offset: f32,
    vintage: f32,
    spread: f32,
    width: f32,
    stage_spread: f64,
    right_offset: f64,
    floor: f64,
    low_cut: f32,
    high_cut: f32,
    blend: f32,
}

impl Knobs {
    fn of(params: &params::Phaser, rate_hz: f64, sample_rate: f64) -> Knobs {
        let open = params.feedback_high_cut_hz >= params::Phaser::HIGH_CUT_OFF_HZ;
        Knobs {
            rate: rate_hz,
            ratio: params.rate_ratio,
            center: params.center_hz,
            range: params.depth_octaves,
            floor: params.sweep_floor_hz,
            feedback: dsp::shaped_feedback(params.feedback),
            drive: if params.vintage { params.drive as f32 } else { 1.0 },
            offset: if params.vintage { VINTAGE_DC_OFFSET } else { 0.0 },
            vintage: if params.vintage { 1.0 } else { 0.0 },
            spread: params.spread as f32,
            width: params.width as f32,
            stage_spread: params.stage_spread,
            right_offset: params.right_offset,
            low_cut: dsp::lp_coeff(params.feedback_low_cut_hz, sample_rate),
            high_cut: if open { 0.0 } else { dsp::lp_coeff(params.feedback_high_cut_hz, sample_rate) },
            blend: params.blend as f32,
        }
    }

    fn approach(&mut self, target: &Knobs, glide: f64) {
        let g = glide as f32;
        self.rate += glide * (target.rate - self.rate);
        self.ratio += glide * (target.ratio - self.ratio);
        self.center += glide * (target.center - self.center);
        self.range += glide * (target.range - self.range);
        self.feedback += g * (target.feedback - self.feedback);
        self.drive += g * (target.drive - self.drive);
        self.offset += g * (target.offset - self.offset);
        self.vintage += g * (target.vintage - self.vintage);
        self.spread += g * (target.spread - self.spread);
        self.width += g * (target.width - self.width);
        self.stage_spread += glide * (target.stage_spread - self.stage_spread);
        if (target.stage_spread - self.stage_spread).abs() < SPREAD_SETTLE {
            self.stage_spread = target.stage_spread;
        }
        self.right_offset += glide * (target.right_offset - self.right_offset);
        self.floor += glide * (target.floor - self.floor);
        self.low_cut += g * (target.low_cut - self.low_cut);
        self.high_cut += g * (target.high_cut - self.high_cut);
        self.blend += g * (target.blend - self.blend);
    }
}

#[derive(Clone, Copy)]
struct Block {
    shape: PhaserShape,
    inv_sr: f64,
    sweep_max_hz: f64,
    square_alpha: f32,
    clamp_stages: bool,
    twin_frames: usize,
    ease: f32,
    cap_knee: f32,
    cap_room: f32,
}

fn lfo(shape: PhaserShape, phase: f64, cycle: i64, seed: u64, square: &mut f32, square_alpha: f32) -> f64 {
    match shape {
        PhaserShape::Sine => dsp::sine(phase),
        PhaserShape::Triangle => dsp::triangle(phase - 0.25),
        PhaserShape::Arc => 2.0 * arc(phase) - 1.0,
        PhaserShape::Round => 2.0 * arc(phase).powi(2) - 1.0,
        PhaserShape::Square => {
            *square += square_alpha * (dsp::square(phase) as f32 - *square);
            f64::from(*square)
        }
        PhaserShape::Step4 => steps(phase, 4),
        PhaserShape::Step8 => steps(phase, 8),
        PhaserShape::SampleHold => dsp::sample_hold(cycle, seed),
    }
}

fn arc(phase: f64) -> f64 {
    let rise = 0.5 * (dsp::triangle(phase - 0.25) + 1.0);
    rise * (2.0 - rise)
}

fn steps(phase: f64, count: usize) -> f64 {
    let p = phase - phase.floor();
    let level = ((p * count as f64) as usize).min(count - 1);
    level as f64 / (count - 1) as f64 * 2.0 - 1.0
}

fn allpass_coeff(hz: f64, inv_sr: f64) -> f32 {
    let t = (PI * hz * inv_sr).tan();
    ((t - 1.0) / (t + 1.0)) as f32
}

fn saturate(x: f32, drive: f32, offset: f32, vintage: f32) -> f32 {
    let lift = (drive * offset).tanh();
    let denom = drive * (1.0 - lift * lift);
    let warm = if denom != 0.0 { ((drive * (x + offset)).tanh() - lift) / denom } else { x };
    (1.0 - vintage) * x + vintage * warm
}

fn capped(x: f32, knee: f32, room: f32) -> f32 {
    let over = x.abs() - knee;
    if over > 0.0 { (knee + room * (over / room).tanh()).copysign(x) } else { x }
}

fn mean(x: [f32; 2], y: [f32; 2]) -> [f32; 2] {
    [0.5 * (x[0] + y[0]), 0.5 * (x[1] + y[1])]
}

fn blend(input: [f32; 2], swept: [f32; 2], share: f32) -> [f32; 2] {
    [(1.0 - share) * input[0] + share * swept[0], (1.0 - share) * input[1] + share * swept[1]]
}

#[derive(Clone, Copy, Default)]
struct Stage {
    x: f32,
    y: f32,
}

impl Stage {
    fn tick(&mut self, input: f32, a: f32) -> f32 {
        let output = a * input + self.x - a * self.y;
        self.x = input;
        self.y = output;
        output
    }
}

fn stored(state: f32, coeff: f32) -> f64 {
    let (state, coeff) = (f64::from(state), f64::from(coeff));
    state * state * coeff / (1.0 - coeff)
}

#[derive(Clone, Copy)]
struct Twin {
    stages: [Stage; MAX_STAGES],
    feedback: f32,
    tone: f32,
    low: f32,
    reach: f32,
    share: f32,
    lifted: f64,
    sum: f64,
    weighed: usize,
    frames: usize,
    level: Option<f64>,
    grown: f64,
}

impl Default for Twin {
    fn default() -> Twin {
        Twin { stages: [Stage::default(); MAX_STAGES], feedback: 1.0, tone: 0.0, low: 0.0, reach: 1.0, share: 1.0, lifted: 0.0, sum: 0.0, weighed: 0, frames: 0, level: None, grown: 0.0 }
    }
}

impl Twin {
    fn ring(&mut self, coeffs: &[f32], knobs: &Knobs, ease: f32) {
        self.share = dsp::settle(self.share, self.reach, ease);
        if knobs.feedback.abs() < TWIN_LEAST {
            return;
        }
        let mut z = (self.reach / TWIN_INSIDE).min(1.0) * knobs.feedback * self.feedback;
        for (stage, a) in self.stages.iter_mut().zip(coeffs) {
            z = stage.tick(z, *a);
        }
        self.tone = dsp::one_pole_lp(z, knobs.high_cut, self.tone);
        self.low = dsp::one_pole_lp(self.tone, knobs.low_cut, self.low);
        self.feedback = self.tone - self.low;
        self.frames += 1;
        if self.frames.is_multiple_of(TWIN_STRIDE) {
            self.weigh(coeffs, knobs);
        }
    }

    fn weigh(&mut self, coeffs: &[f32], knobs: &Knobs) {
        let (mut held, mut open) = (stored(self.tone, knobs.high_cut) + stored(self.low, knobs.low_cut), 1.0);
        for (stage, a) in self.stages.iter().zip(coeffs) {
            let a = f64::from(*a);
            let v = f64::from(stage.x) - a * f64::from(stage.y);
            held += v * v / (1.0 - a * a);
            open *= 1.0 - a * a;
        }
        let level = held.ln() + open.ln() / coeffs.len() as f64 - 2.0 * self.lifted;
        if !level.is_finite() {
            *self = Twin { reach: self.reach, share: self.share, ..Twin::default() };
            return;
        }
        self.sum += level;
        self.weighed += 1;
        if !(1.0 / TWIN_SPAN..TWIN_SPAN).contains(&held) {
            self.scale(held.powf(-0.5) as f32);
        }
    }

    fn scale(&mut self, by: f32) {
        for stage in &mut self.stages {
            stage.x *= by;
            stage.y *= by;
        }
        self.feedback *= by;
        self.tone *= by;
        self.low *= by;
        self.lifted += f64::from(by).ln();
    }

    fn judge(&mut self, block: &Block) {
        if self.frames < block.twin_frames || self.weighed == 0 {
            return;
        }
        let level = self.sum / self.weighed as f64;
        if let Some(before) = self.level {
            let rose = level - before;
            self.grown = (self.grown + rose).max(0.0);
            if self.grown > TWIN_GROWN {
                self.reach = (self.reach / TWIN_INSIDE).min(1.0) * TWIN_INSIDE * (-0.5 * rose).exp() as f32;
                self.grown = 0.0;
            } else if self.grown == 0.0 {
                self.reach = (self.reach * (TWIN_REGAIN * self.frames as f64 * block.inv_sr).exp() as f32).min(1.0);
            }
        }
        self.level = Some(level + 2.0 * self.lifted);
        (self.lifted, self.sum, self.weighed, self.frames) = (0.0, 0.0, 0, 0);
    }
}

#[derive(Clone, Copy, Default)]
struct Line {
    stages: [Stage; MAX_STAGES],
    feedback: f32,
    tone: f32,
    hp: OnePoleHp,
    square: f32,
    twin: Twin,
}

impl Line {
    fn run(&mut self, input: f32, count: usize, hz: f64, knobs: &Knobs, block: &Block) -> f32 {
        let mut s = input + self.twin.share * (knobs.feedback * self.feedback);
        let floor = if block.clamp_stages { knobs.floor } else { SWEEP_MIN_HZ };
        let mut coeffs = [0.0; MAX_STAGES];
        if knobs.stage_spread == 0.0 {
            coeffs[..count].fill(allpass_coeff(if block.clamp_stages { hz.clamp(floor, block.sweep_max_hz) } else { hz }, block.inv_sr));
        } else {
            let lowest = -0.5 * (count - 1) as f64 * knobs.stage_spread;
            for (k, a) in coeffs[..count].iter_mut().enumerate() {
                let stage_hz = (hz * (lowest + k as f64 * knobs.stage_spread).exp2()).clamp(floor, block.sweep_max_hz);
                *a = allpass_coeff(stage_hz, block.inv_sr);
            }
        }
        for (stage, a) in self.stages[..count].iter_mut().zip(coeffs) {
            s = stage.tick(s, a);
        }
        self.twin.ring(&coeffs[..count], knobs, block.ease);
        self.tone = dsp::one_pole_lp(s, knobs.high_cut, self.tone);
        self.feedback = self.hp.tick(saturate(self.tone, knobs.drive, knobs.offset, knobs.vintage), knobs.low_cut);
        s
    }
}

#[derive(Clone, Copy, Default)]
struct Cascade {
    phase: f64,
    cycle: i64,
    lines: [Line; 2],
}

impl Cascade {
    fn run(&mut self, input: [f32; 2], count: usize, turn: f64, seed: u64, knobs: &Knobs, block: &Block) -> [f32; 2] {
        let mut phase = [self.phase, self.phase + 0.5 * f64::from(knobs.spread)];
        let mut cycle = [self.cycle; 2];
        if phase[1] >= 1.0 {
            phase[1] -= 1.0;
            cycle[1] += 1;
        }
        self.phase += turn;
        let turned = self.phase >= 1.0;
        if turned {
            self.phase -= self.phase.floor();
            self.cycle += 1;
        }
        let mut output = [0.0; 2];
        for ch in 0..2 {
            let line = &mut self.lines[ch];
            let lfo = lfo(block.shape, phase[ch], cycle[ch], seed, &mut line.square, block.square_alpha);
            let mut octaves = lfo * knobs.range * 0.5;
            if ch == 1 {
                octaves += knobs.right_offset;
            }
            let swept = knobs.center * octaves.exp2();
            let hz = if block.clamp_stages { swept } else { swept.clamp(SWEEP_MIN_HZ, block.sweep_max_hz) };
            output[ch] = line.run(input[ch], count, hz, knobs, block);
            if turned {
                line.twin.judge(block);
            }
        }
        output
    }

    fn restart(&mut self, (cycle, phase): (i64, f64)) {
        self.phase = phase;
        self.cycle = cycle;
        for line in &mut self.lines {
            line.square = 0.0;
        }
    }
}

#[derive(Clone, Copy)]
struct Circuit {
    topology: Topology,
    a: Cascade,
    b: Cascade,
}

impl Circuit {
    fn new(topology: Topology) -> Circuit {
        Circuit { topology, a: Cascade::default(), b: Cascade::default() }
    }

    fn rewire(&mut self, topology: Topology) {
        let old = self.topology;
        for (cascade, was, now) in [(&mut self.a, old.stages_a, topology.stages_a), (&mut self.b, old.stages_b, topology.stages_b)] {
            if now > was {
                for line in &mut cascade.lines {
                    line.stages[was..now].fill(Stage::default());
                }
            }
        }
        if old.routing == Routing::Single && topology.routing != Routing::Single {
            self.b = Cascade { phase: self.a.phase, cycle: self.a.cycle, ..Cascade::default() };
        }
        self.topology = topology;
    }

    fn restart(&mut self, a: (i64, f64), b: (i64, f64)) {
        self.a.restart(a);
        self.b.restart(b);
    }

    fn run(&mut self, input: [f32; 2], knobs: &Knobs, block: &Block) -> [f32; 2] {
        let topology = self.topology;
        let a = self.a.run(input, topology.stages_a, knobs.rate * block.inv_sr, SEED_A, knobs, block);
        if topology.routing == Routing::Single {
            return blend(input, a, knobs.blend);
        }
        let series = topology.routing == Routing::Series;
        let b = self.b.run(if series { a } else { input }, topology.stages_b, knobs.rate * knobs.ratio * block.inv_sr, SEED_B, knobs, block);
        blend(input, if series { b } else { mean(a, b) }, knobs.blend)
    }
}

pub struct Phaser {
    sample_rate: f64,
    block: Block,
    glide: f64,
    fade_len: usize,
    circuit: Circuit,
    outgoing: Circuit,
    fade_left: usize,
    knobs: Option<Knobs>,
    mix: Mix,
    tickle: f32,
    restarting: bool,
    joining: bool,
}

impl Phaser {
    pub(super) fn clear(&mut self) {
        self.circuit = Circuit::new(Topology::of(&params::Phaser::DEFAULT));
        self.outgoing = self.circuit;
        self.fade_left = 0;
        self.knobs = None;
        self.mix = Mix::new(self.sample_rate);
        self.tickle = TICKLE;
        self.restarting = true;
    }
}

impl Unit for Phaser {
    type Params = params::Phaser;

    fn new(sample_rate: f64, _: usize) -> Phaser {
        let circuit = Circuit::new(Topology::of(&params::Phaser::DEFAULT));
        Phaser {
            sample_rate,
            block: Block {
                shape: PhaserShape::Sine,
                inv_sr: 1.0 / sample_rate,
                sweep_max_hz: SWEEP_MAX_HZ.min(sample_rate * 0.5 - NYQUIST_GUARD_HZ),
                square_alpha: dsp::smoothing(SQUARE_SECONDS, sample_rate) as f32,
                clamp_stages: false,
                twin_frames: (TWIN_SECONDS * sample_rate) as usize,
                ease: dsp::smoothing(EASE_SECONDS, sample_rate) as f32,
                cap_knee: dsp::db_to_gain(CAP_KNEE_DB) as f32,
                cap_room: (dsp::db_to_gain(CAP_TOP_DB) - dsp::db_to_gain(CAP_KNEE_DB)) as f32,
            },
            glide: dsp::smoothing(GLIDE_SECONDS, sample_rate),
            fade_len: FADE_MIN_FRAMES.max((FADE_SECONDS * sample_rate) as usize),
            circuit,
            outgoing: circuit,
            fade_left: 0,
            knobs: None,
            mix: Mix::new(sample_rate),
            tickle: TICKLE,
            restarting: true,
            joining: false,
        }
    }

    fn restart(&mut self) {
        self.restarting = true;
        self.joining = true;
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], params: &params::Phaser, wet: f32, clock: &Clock) {
        dsp::sanitize(l, r);
        let wet = wet.clamp(0.0, 1.0);
        if wet <= 0.0 && self.mix.z <= 0.0 {
            self.restarting = true;
            self.joining = false;
            return;
        }
        let topology = Topology::of(params);
        if topology != self.circuit.topology {
            self.outgoing = self.circuit;
            self.circuit.rewire(topology);
            self.fade_left = self.fade_len;
        }
        let target = Knobs::of(params, clock.rate(params.sync, params.division, params.rate_hz), self.sample_rate);
        let mut knobs = self.knobs.unwrap_or(target);
        if self.restarting {
            self.restarting = false;
            let turned = turns(clock, params.sync, params.division, params.rate_hz);
            let (a, b) = (begun(params.start_phase, turned), begun(params.start_phase, turned * params.rate_ratio));
            if self.joining && self.knobs.is_some() && self.fade_left == 0 {
                self.outgoing = self.circuit;
                self.fade_left = self.fade_len;
            } else {
                self.outgoing.restart(a, b);
            }
            self.circuit.restart(a, b);
        }
        self.joining = false;
        let block = Block { shape: params.shape, clamp_stages: params.stage_clamp, ..self.block };
        let (glide, fade_len) = (self.glide, self.fade_len);
        let (mut tickle, mut fade_left) = (self.tickle, self.fade_left);
        for i in 0..l.len().min(r.len()) {
            if wet <= 0.0 && self.mix.z <= 0.0 {
                self.restarting = true;
                break;
            }
            tickle = -tickle;
            knobs.approach(&target, glide);
            let dry = [l[i], r[i]];
            let input = [dry[0] + tickle, dry[1] + tickle];
            let mut phased = self.circuit.run(input, &knobs, &block);
            if fade_left > 0 {
                let fading = self.outgoing.run(input, &knobs, &block);
                let gain = (fade_len - fade_left) as f32 / fade_len as f32;
                for ch in 0..2 {
                    phased[ch] = gain * phased[ch] + (1.0 - gain) * fading[ch];
                }
                fade_left -= 1;
            }
            if (knobs.width - 1.0).abs() > dsp::WIDTH_UNITY_EPS {
                let (left, right) = dsp::widen(phased[0], phased[1], knobs.width);
                phased = [left, right];
            }
            let mix = self.mix.tick(wet);
            let kept = 1.0 - mix + params.dry as f32 * mix;
            l[i] = dsp::finite(kept * dry[0] + mix * capped(phased[0], block.cap_knee, block.cap_room));
            r[i] = dsp::finite(kept * dry[1] + mix * capped(phased[1], block.cap_knee, block.cap_room));
        }
        self.knobs = Some(knobs);
        self.tickle = tickle;
        self.fade_left = fade_left;
    }
}

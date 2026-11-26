use super::{CAPACITY, OSCS, mix_compensation};
use crate::osc::{Cycle, shaped};
use crate::params::{Analog, Filter, Noise, Osc, WAVES, Wave};
use crate::voice::Memo;
use sdk::dsp::noise::NoiseColor;
use sdk::dsp::unison::{MAX_UNISON, Spread};
use sdk::dsp::{Halfband, Smooth, tanh_integral};
use sdk::face::Drawn;
use std::f64::consts::PI;

const DRIVE: f64 = 0.1826;
const DRIVE_RISE: f64 = 3.56;
const LEAST_DRIVE: f64 = 1e-9;
const LEAN_BIAS: f64 = 0.565;
const PRE_CEILING: f64 = 0.61;
const POST_CEILING: f64 = 0.47;
const GLIDE_SECONDS: f64 = 0.015;
const FADE_SECONDS: f64 = 0.015;
const TAIL_SECONDS: f64 = 0.010;
const BLOCK_HZ: f64 = 10.0;
const ASLEEP: f64 = 1e-4;
const SETTLED: f64 = 1e-9;
const LEAST_SPAN: f64 = 1e-5;
const TILT_FROM: f64 = 2.0;
const TILT_TO: f64 = 16.0;
const DULL: [f64; 3] = tilt(TILT_FROM, TILT_TO);
const BRIGHT: [f64; 3] = tilt(TILT_TO, TILT_FROM);
const PRE: usize = 0;
const POST: usize = 1;
const CUT: usize = 2;
const PHASES: usize = 128;
const BANDS: usize = 32;
const QUIET: f64 = 1e-3;
const WHITE_POWER: f64 = 0.334;
const PINK_POWER: f64 = 0.735;
const BROWN_POWER: f64 = 2.61;
const PINK_KNEE_HZ: f64 = 2.0;
const BROWN_FLOOR_HZ: f64 = 2.0;
const BROWN_ROLL_HZ: f64 = 10.0;
const TONE_LOWEST_HZ: f64 = 20.0;
const TONE_OCTAVES: f64 = 10.0;
const TONE_IDLE: f64 = 0.01;
const TONE_GUARD: f64 = 0.49;
const CORNER_GUARD: f64 = 0.45;

const fn tilt(from: f64, to: f64) -> [f64; 3] {
    let scale = 1.0 + 1.0 / from;
    [(1.0 + 1.0 / to) / scale, (1.0 - 1.0 / to) / scale, (1.0 - 1.0 / from) / scale]
}

fn raised(fade: f64) -> f64 {
    if fade <= 0.0 {
        0.0
    } else if fade >= 1.0 {
        1.0
    } else {
        0.5 - 0.5 * (PI * fade).cos()
    }
}

fn pole(hz: f64, turn: f64) -> f64 {
    let rise = (turn * hz).tan();
    rise / (1.0 + rise)
}

fn low(held: &mut f64, x: f64, pole: f64) -> f64 {
    let rise = (x - *held) * pole;
    let low = rise + *held;
    *held = low + rise;
    low
}

fn blend(dry: f64, wet: f64, mix: f64) -> f64 {
    if mix >= 1.0 { wet } else { dry + mix * (wet - dry) }
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct HeatPlan {
    pub(super) pre: bool,
    pub(super) after: bool,
    post: bool,
    cut: bool,
    woke: [u32; 3],
    block: f64,
    tail: f64,
}

#[derive(Clone, Copy, Debug)]
struct Knob {
    near: Smooth,
    far: Smooth,
    fade: f64,
    wakes: u32,
}

impl Knob {
    fn at(aim: f64, sample_rate: f64) -> Knob {
        let held = Smooth::new(aim, GLIDE_SECONDS, sample_rate);
        Knob { near: held, far: held, fade: if aim > 0.0 { 1.0 } else { 0.0 }, wakes: 1 }
    }

    fn snap(&mut self, aim: f64) {
        self.near.snap(aim);
        self.far.snap(aim);
        self.fade = if aim > 0.0 { 1.0 } else { 0.0 };
    }

    fn awake(&self, aim: f64) -> bool {
        aim > 0.0 || self.fade > 0.0
    }

    fn tick(&mut self, aim: f64, span: f64, pace: f64) -> (f64, f64) {
        let near = self.near.tick(aim);
        let far = self.far.tick(near);
        if (aim - near).abs() < SETTLED * span && (aim - far).abs() < SETTLED * span {
            self.near.snap(aim);
            self.far.snap(aim);
        }
        if aim > 0.0 || far > ASLEEP * span {
            if self.fade <= 0.0 {
                self.wakes = self.wakes.wrapping_add(1);
            }
            self.fade = (self.fade + pace).min(1.0);
        } else {
            self.fade = (self.fade - pace).max(0.0);
            if self.fade <= 0.0 {
                self.near.snap(0.0);
                self.far.snap(0.0);
            }
        }
        (self.far.z, raised(self.fade))
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Curve {
    drive: f64,
    norm: f64,
    gain: f64,
    mix: f64,
}

#[derive(Clone, Copy, Debug, Default)]
struct Frame {
    pre: Curve,
    post: Curve,
    bias: f64,
    level: f64,
    blocking: f64,
    cut: f64,
    cut_mix: f64,
}

impl Frame {
    fn shapes_after(&self, plan: &HeatPlan) -> bool {
        plan.post && self.post.mix > 0.0 || plan.cut && self.cut_mix > 0.0
    }
}

pub(super) struct Sources<'a> {
    pub(super) oscs: &'a [Osc; OSCS],
    pub(super) spread: &'a [Spread; OSCS],
    pub(super) cycles: &'a [Cycle; OSCS],
    pub(super) noise: &'a Noise,
    pub(super) lifts: [f64; OSCS],
    pub(super) noise_lift: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Outline {
    shapes: [Wave; WAVES],
    levels: [f64; WAVES],
    width: f64,
    drawn: Option<Drawn>,
}

impl Outline {
    fn of(osc: &Osc) -> Outline {
        let width = if osc.pure() { osc.pulse_width.clamp(0.05, 0.95) } else { 0.5 };
        Outline { shapes: osc.shapes, levels: osc.levels, width, drawn: osc.shapes.contains(&Wave::Drawn).then_some(osc.points) }
    }

    fn power(&self, cycle: &Cycle) -> f64 {
        let heard = |phase: f64| -> f64 { self.shapes.iter().zip(self.levels).filter(|(_, level)| *level > 0.0).map(|(wave, level)| level * shaped(*wave, phase, 0.0, self.width, 1.0, cycle)).sum() };
        (0..PHASES).map(|step| heard((step as f64 + 0.5) / PHASES as f64).powi(2)).sum::<f64>() / PHASES as f64
    }
}

#[derive(Debug)]
struct Gauge {
    rate: f64,
    one_saw: f64,
    outlines: [Option<Outline>; OSCS],
    powers: [f64; OSCS],
    bands: [(f64, f64); BANDS],
}

impl Gauge {
    fn new(sample_rate: f64) -> Gauge {
        let bands = std::array::from_fn(|band| {
            let hz = (0.5 * sample_rate).powf((band as f64 + 0.5) / BANDS as f64);
            (hz, (PI * hz / sample_rate).tan().powi(4))
        });
        Gauge { rate: sample_rate, one_saw: Outline::of(&Osc::DEFAULT).power(&Cycle::flat()), outlines: [None; OSCS], powers: [0.0; OSCS], bands }
    }

    fn strength(&mut self, sources: &Sources) -> f64 {
        let mut power = 0.0;
        for (k, osc) in sources.oscs.iter().enumerate() {
            let scale = mix_compensation(osc.levels) * (osc.level + sources.lifts[k]).clamp(0.0, 1.0);
            if scale > 0.0 {
                let outline = Outline::of(osc);
                if self.outlines[k] != Some(outline) {
                    self.outlines[k] = Some(outline);
                    self.powers[k] = outline.power(&sources.cycles[k]);
                }
                let copies = osc.unison.clamp(1, MAX_UNISON);
                let spread = &sources.spread[k];
                let sides: f64 = spread.gain_l[..copies].iter().chain(&spread.gain_r[..copies]).map(|gain| gain * gain).sum();
                power += scale * scale * self.powers[k] * 0.5 * sides;
            }
        }
        let noise = sources.noise;
        let reached = noise.level + sources.noise_lift;
        if noise.on && reached > 0.0 {
            power += reached * reached * self.hiss(noise);
        }
        let strength = (power / self.one_saw).sqrt();
        if strength < QUIET { 1.0 } else { strength }
    }

    fn hiss(&self, noise: &Noise) -> f64 {
        let corner = |hz: f64, least: f64, guard: f64| (PI * hz.clamp(least, guard * self.rate) / self.rate).tan().powi(4);
        let tone = (noise.tone.abs() >= TONE_IDLE).then(|| {
            let octaves = if noise.tone < 0.0 { -noise.tone } else { 1.0 - noise.tone } * TONE_OCTAVES;
            (corner(TONE_LOWEST_HZ * octaves.exp2(), 1.0, TONE_GUARD), noise.tone < 0.0)
        });
        let high = (noise.high_pass_hz > Filter::MIN_HZ).then(|| (corner(noise.high_pass_hz, Filter::MIN_HZ, CORNER_GUARD), true));
        let low = (noise.low_pass_hz < Filter::MAX_HZ).then(|| (corner(noise.low_pass_hz, Filter::MIN_HZ, CORNER_GUARD), false));
        let (mut kept, mut whole) = (0.0, 0.0);
        for (hz, warped) in self.bands {
            let weight = match noise.color {
                NoiseColor::White => hz,
                NoiseColor::Pink => hz / (hz + PINK_KNEE_HZ),
                NoiseColor::Brown => hz * hz * hz / ((hz * hz + BROWN_FLOOR_HZ * BROWN_FLOOR_HZ) * (hz * hz + BROWN_ROLL_HZ * BROWN_ROLL_HZ)),
            };
            let passed: f64 = [tone, high, low].into_iter().flatten().map(|(corner, highs)| if highs { warped } else { corner } / (corner + warped)).product();
            kept += weight * passed;
            whole += weight;
        }
        let plain = match noise.color {
            NoiseColor::White => WHITE_POWER,
            NoiseColor::Pink => PINK_POWER,
            NoiseColor::Brown => BROWN_POWER,
        };
        plain * kept / whole
    }
}

#[derive(Debug)]
pub(super) struct Dials {
    pre: Knob,
    post: Knob,
    lean: Knob,
    cut: Knob,
    strength: Knob,
    gauge: Gauge,
    live: bool,
    fade: f64,
    tail: f64,
    block: f64,
    turn: f64,
    frames: Vec<Frame>,
}

#[derive(Clone, Copy, Debug)]
struct Aims {
    pre: f64,
    post: f64,
    lean: f64,
    cut: f64,
}

impl Aims {
    fn of(analog: &Analog) -> Aims {
        let analog = &analog.heard();
        Aims { pre: analog.heat_pre.clamp(0.0, 1.0), post: analog.heat_post.clamp(0.0, 1.0), lean: analog.lean.clamp(0.0, 1.0), cut: analog.ac_hz.clamp(0.0, Analog::MAX_AC_HZ) }
    }
}

impl Dials {
    pub(super) fn new(sample_rate: f64, analog: &Analog) -> Dials {
        let turn = PI / sample_rate;
        let aim = Aims::of(analog);
        Dials {
            pre: Knob::at(aim.pre, sample_rate),
            post: Knob::at(aim.post, sample_rate),
            lean: Knob::at(aim.lean, sample_rate),
            cut: Knob::at(aim.cut, sample_rate),
            strength: Knob::at(1.0, sample_rate),
            gauge: Gauge::new(sample_rate),
            live: false,
            fade: 1.0 / (FADE_SECONDS * sample_rate),
            tail: 1.0 / (TAIL_SECONDS * sample_rate),
            block: pole(BLOCK_HZ, turn),
            turn,
            frames: vec![Frame::default(); CAPACITY],
        }
    }

    pub(super) fn plan(&mut self, analog: &Analog, sources: &Sources, n: usize) -> HeatPlan {
        let aim = Aims::of(analog);
        let (pre, post, cut) = (self.pre.awake(aim.pre), self.post.awake(aim.post), self.cut.awake(aim.cut));
        if !(pre || post || cut) {
            self.live = false;
            return HeatPlan::default();
        }
        let (fade, turn) = (self.fade, self.turn);
        let mixed = if pre || post { self.gauge.strength(sources) } else { 1.0 };
        let (mut level, mut corner, mut pre_drive, mut post_drive) = (Memo::default(), Memo::default(), Memo::default(), Memo::default());
        for frame in &mut self.frames[..n] {
            if self.pre.awake(aim.pre) || self.post.awake(aim.post) {
                if !self.live {
                    self.lean.snap(aim.lean);
                    self.strength.snap(mixed);
                    self.live = true;
                }
                let (lean, blocking) = self.lean.tick(aim.lean, 1.0, fade);
                let (strength, _) = self.strength.tick(mixed, 1.0, fade);
                frame.bias = LEAN_BIAS * lean;
                frame.level = level.of(frame.bias, f64::tanh);
                frame.blocking = blocking;
                let slope = 1.0 - frame.level * frame.level;
                frame.pre = curve(&mut self.pre, aim.pre, slope, PRE_CEILING, strength, fade, &mut pre_drive);
                frame.post = curve(&mut self.post, aim.post, slope, POST_CEILING, strength, fade, &mut post_drive);
            } else {
                self.live = false;
                (frame.pre.mix, frame.post.mix) = (0.0, 0.0);
            }
            if cut {
                let (hz, mix) = self.cut.tick(aim.cut, Analog::MAX_AC_HZ, fade);
                frame.cut = corner.of(hz, |hz| pole(hz, turn));
                frame.cut_mix = mix;
            }
        }
        HeatPlan { pre, after: post || cut, post, cut, woke: [self.pre.wakes, self.post.wakes, self.cut.wakes], block: self.block, tail: self.tail }
    }
}

fn curve(knob: &mut Knob, aim: f64, slope: f64, ceiling: f64, strength: f64, pace: f64, push: &mut Memo<f64, f64>) -> Curve {
    let (amount, mix) = knob.tick(aim, 1.0, pace);
    let push = push.of(amount, |amount| (DRIVE * (DRIVE_RISE * amount).exp_m1()).max(LEAST_DRIVE));
    let reach = ceiling * push * slope;
    let drive = push / strength;
    Curve { drive, norm: 1.0 / (drive * slope), gain: (1.0 + reach * reach).sqrt(), mix }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Shape {
    drive: f64,
    bias: f64,
    level: f64,
    norm: f64,
}

impl Shape {
    fn at(&self, x: f64) -> f64 {
        ((self.drive * x + self.bias).tanh() - self.level) * self.norm
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Tilt {
    fed: f64,
    gave: f64,
}

impl Tilt {
    fn run(&mut self, x: f64, [now, before, back]: [f64; 3]) -> f64 {
        let y = now * x + before * self.fed - back * self.gave;
        self.fed = x;
        self.gave = y;
        y
    }

    fn settle(&mut self, x: f64) {
        self.fed = x;
        self.gave = x;
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Twin {
    even: Halfband,
    odd: Halfband,
    held: f32,
}

impl Twin {
    fn run(&mut self, first: f32, second: f32) -> (f32, f32) {
        let (even, between) = self.even.upsample(first);
        let (odd, late) = self.odd.upsample(second);
        let out = (0.5 * (even + self.held), 0.5 * (odd + between));
        self.held = late;
        out
    }

    fn settle(&mut self, x: f32) {
        self.even.settle(x);
        self.odd.settle(x);
        self.held = x;
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Lane {
    dull: Tilt,
    bright: Tilt,
    up: [Halfband; 2],
    down: [Halfband; 2],
    rising: Twin,
    falling: Twin,
    shape: Shape,
    last: f64,
    area: f64,
    offset: f64,
}

impl Lane {
    fn settle(&mut self, x: f64, shape: &Shape) {
        let (thin, bent) = (x as f32, shape.at(x));
        self.dull.settle(x);
        self.up[0].settle(thin);
        self.rising.settle(thin);
        self.up[1].settle(thin);
        self.shape = *shape;
        self.last = x;
        self.area = tanh_integral(shape.drive * x + shape.bias);
        self.down[1].settle(bent as f32);
        self.falling.settle(bent as f32);
        self.down[0].settle(bent as f32);
        self.bright.settle(bent);
        self.offset = 0.0;
    }

    fn run(&mut self, x: f64, shape: &Shape, block: f64, blocking: f64) -> f64 {
        if self.shape != *shape {
            self.shape = *shape;
            self.area = tanh_integral(shape.drive * self.last + shape.bias);
        }
        let Lane { dull, bright, up, down, rising, falling, last, area, offset, .. } = self;
        let mut bend = |u: f32| {
            let u = f64::from(u);
            let (from, to) = (shape.drive * *last + shape.bias, shape.drive * u + shape.bias);
            let under = tanh_integral(to);
            let mean = if (to - from).abs() < LEAST_SPAN { (0.5 * (from + to)).tanh() } else { (under - *area) / (to - from) };
            let out = (mean - shape.level) * shape.norm + 0.5 * (u - *last);
            *last = u;
            *area = under;
            out as f32
        };
        let (first, second) = up[0].upsample(dull.run(x, DULL) as f32);
        let (first, second) = rising.run(first, second);
        let mut pair = |x: f32| {
            let (first, second) = up[1].upsample(x);
            let (first, second) = (bend(first), bend(second));
            down[1].downsample(first, second)
        };
        let (first, second) = (pair(first), pair(second));
        let (first, second) = falling.run(first, second);
        let wet = bright.run(f64::from(down[0].downsample(first, second)), BRIGHT);
        wet - blocking * low(offset, wet, block)
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Stage {
    lanes: [Lane; 2],
    wide: bool,
    woke: u32,
}

impl Stage {
    fn run(&mut self, woke: u32, curve: &Curve, frame: &Frame, block: f64, x_l: f64, x_r: f64) -> (f64, f64) {
        if curve.mix <= 0.0 {
            return (x_l, x_r);
        }
        let shape = Shape { drive: curve.drive, bias: frame.bias, level: frame.level, norm: curve.norm };
        if !self.wide && x_l.to_bits() != x_r.to_bits() {
            self.lanes[1] = self.lanes[0];
            self.wide = true;
        }
        if self.woke != woke {
            self.woke = woke;
            self.lanes[0].settle(x_l, &shape);
            if self.wide {
                self.lanes[1].settle(x_r, &shape);
            }
        }
        let l = blend(x_l, curve.gain * self.lanes[0].run(x_l, &shape, block, frame.blocking), curve.mix);
        if !self.wide {
            return (l, l);
        }
        (l, blend(x_r, curve.gain * self.lanes[1].run(x_r, &shape, block, frame.blocking), curve.mix))
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct LowCut {
    held: [f64; 2],
    wide: bool,
    woke: u32,
}

impl LowCut {
    fn run(&mut self, woke: u32, pole: f64, mix: f64, x_l: f64, x_r: f64) -> (f64, f64) {
        if mix <= 0.0 {
            return (x_l, x_r);
        }
        if self.woke != woke {
            self.woke = woke;
            self.held = [0.0; 2];
        }
        if !self.wide && x_l.to_bits() != x_r.to_bits() {
            self.held[1] = self.held[0];
            self.wide = true;
        }
        let l = x_l - mix * low(&mut self.held[0], x_l, pole);
        if !self.wide {
            return (l, l);
        }
        (l, x_r - mix * low(&mut self.held[1], x_r, pole))
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Heat {
    pre: Stage,
    post: Stage,
    cut: LowCut,
    shut: f64,
    lingers: bool,
}

impl Heat {
    pub(crate) fn reset(&mut self) {
        *self = Heat::default();
    }

    pub(crate) fn lingers(&self) -> bool {
        self.lingers
    }

    pub(super) fn spent(&mut self, plan: &HeatPlan, dials: &Dials, i: usize) -> bool {
        self.lingers = plan.after && self.shut < 1.0 && dials.frames[i].shapes_after(plan);
        !self.lingers
    }

    pub(super) fn pre(&mut self, plan: &HeatPlan, dials: &Dials, i: usize, x_l: f64, x_r: f64) -> (f64, f64) {
        let frame = &dials.frames[i];
        self.pre.run(plan.woke[PRE], &frame.pre, frame, plan.block, x_l, x_r)
    }

    pub(super) fn after(&mut self, plan: &HeatPlan, dials: &Dials, i: usize, x_l: f64, x_r: f64) -> (f64, f64) {
        let frame = &dials.frames[i];
        let (mut l, mut r) = (x_l, x_r);
        if plan.post {
            let from_rest = self.post.woke == 0 && frame.post.mix >= 1.0;
            if from_rest {
                self.post.woke = plan.woke[POST];
            }
            (l, r) = self.post.run(plan.woke[POST], &frame.post, frame, plan.block, l, r);
        }
        if plan.cut {
            (l, r) = self.cut.run(plan.woke[CUT], frame.cut, frame.cut_mix, l, r);
        }
        if self.lingers {
            self.shut = (self.shut + plan.tail).min(1.0);
            let open = 1.0 - raised(self.shut);
            (l, r) = (l * open, r * open);
        }
        (l, r)
    }
}

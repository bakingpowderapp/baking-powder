use sdk::dsp;
use sdk::dsp::svf::{self, BiquadCoeffs};
use crate::band_pass::BandPass;
use crate::compressor::{gain_reduction, slope_of};
use crate::params::{Channel, EQ_BANDS, EqBand, Shape};
use crate::silence::Silence;
use crate::{Clock, Unit, params};
use std::f64::consts::PI;

const RAMP_SECONDS: f64 = 0.005;
const PLUS_STAGES: usize = 4;
const TICKLE: f32 = 1e-25;
const SHELF_Q: f64 = std::f64::consts::FRAC_1_SQRT_2;
const HOP: usize = 16;
const SURROUND_Q: f64 = 0.7;
const SURROUND_SHARE: f64 = 0.5;
const SURROUND_FLOOR: f64 = 1e-3;
const POWER_EPS: f64 = 1e-30;
const SILENCE: f64 = 1e-10;
const RELEASE_PERIODS: f64 = 10.0;
const AVERAGE_CYCLES: f64 = 4.0;
const AVERAGE_LONGEST: f64 = 0.1;

#[derive(Clone, Copy)]
struct Coeffs {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl Coeffs {
    const FLAT: Coeffs = Coeffs { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 };
    const ZERO: Coeffs = Coeffs { b0: 0.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 };

    fn narrow(c: BiquadCoeffs) -> Coeffs {
        Coeffs { b0: c.b0 as f32, b1: c.b1 as f32, b2: c.b2 as f32, a1: c.a1 as f32, a2: c.a2 as f32 }
    }

    fn zip(self, other: Coeffs, f: impl Fn(f32, f32) -> f32) -> Coeffs {
        Coeffs { b0: f(self.b0, other.b0), b1: f(self.b1, other.b1), b2: f(self.b2, other.b2), a1: f(self.a1, other.a1), a2: f(self.a2, other.a2) }
    }
}

fn stages(shape: Shape) -> usize {
    if shape.is_plus() { PLUS_STAGES } else { 1 }
}

fn clamped(band: &EqBand, sample_rate: f64) -> (f64, f64) {
    (band.hz.min((0.45 * sample_rate).min(20_000.0)).max(20.0), band.q.clamp(0.1, band.shape.max_q()))
}

pub fn listen(band: &EqBand, sample_rate: f64) -> BiquadCoeffs {
    let (hz, q) = clamped(band, sample_rate);
    match band.shape {
        Shape::HighPass | Shape::HighPassPlus => svf::low_pass(hz, q, sample_rate),
        Shape::LowPass | Shape::LowPassPlus => svf::high_pass(hz, q, sample_rate),
        Shape::LowShelf => svf::low_pass(hz, SHELF_Q, sample_rate),
        Shape::LowShelfQ => svf::low_pass(hz, q, sample_rate),
        Shape::HighShelf => svf::high_pass(hz, SHELF_Q, sample_rate),
        Shape::HighShelfQ => svf::high_pass(hz, q, sample_rate),
        Shape::Bell | Shape::Notch => svf::band_pass(hz, q, sample_rate),
    }
}

fn coefficients(band: &EqBand, scale: f64, sample_rate: f64) -> Coeffs {
    shaped(band, band.gain_db * scale, sample_rate)
}

pub fn design(band: &EqBand, gain_db: f64, sample_rate: f64) -> BiquadCoeffs {
    let (hz, q) = clamped(band, sample_rate);
    match band.shape {
        Shape::HighPass | Shape::HighPassPlus => svf::high_pass(hz, q, sample_rate),
        Shape::LowShelf => svf::low_shelf(gain_db, hz, sample_rate),
        Shape::LowShelfQ => svf::low_shelf_q(gain_db, hz, q, sample_rate),
        Shape::Bell => svf::bell(hz, q, gain_db, sample_rate),
        Shape::Notch => svf::notch(hz, q, sample_rate),
        Shape::HighShelf => svf::high_shelf(gain_db, hz, sample_rate),
        Shape::HighShelfQ => svf::high_shelf_q(gain_db, hz, q, sample_rate),
        Shape::LowPass | Shape::LowPassPlus => svf::low_pass(hz, q, sample_rate),
    }
}

fn shaped(band: &EqBand, gain_db: f64, sample_rate: f64) -> Coeffs {
    Coeffs::narrow(design(band, gain_db, sample_rate))
}

fn surround_q(q: f64) -> f64 {
    SURROUND_Q.min(q * SURROUND_SHARE)
}

fn noise_gain(c: &BiquadCoeffs) -> f64 {
    let (a1, a2) = (c.a1, c.a2);
    let d = (1.0 - a2) * ((1.0 + a2) * (1.0 + a2) - a1 * a1);
    let r0 = (1.0 + a2) / d;
    let r1 = -a1 / d;
    let r2 = (a1 * a1 - a2 * (1.0 + a2)) / d;
    (c.b0 * c.b0 + c.b1 * c.b1 + c.b2 * c.b2) * r0 + 2.0 * (c.b0 * c.b1 + c.b1 * c.b2) * r1 + 2.0 * c.b0 * c.b2 * r2
}

#[derive(Clone, Copy, Default)]
struct Stage {
    s1: f32,
    s2: f32,
}

impl Stage {
    fn tick(&mut self, x: f32, c: Coeffs) -> f32 {
        let y = c.b0 * x + self.s1;
        self.s1 = c.b1 * x - c.a1 * y + self.s2;
        self.s2 = c.b2 * x - c.a2 * y;
        y
    }
}

fn cascade(stages: &mut [Stage], x: f32, c: Coeffs) -> f32 {
    let mut y = x;
    for stage in stages {
        y = stage.tick(y, c);
    }
    y
}

struct Block {
    scale: f64,
    sample_rate: f64,
    frames: usize,
    ramp: f32,
    fresh: bool,
    rescaled: bool,
}

#[derive(Clone, Copy)]
struct Law {
    power: f64,
    attack: f64,
    release: f64,
    threshold_db: f64,
    slope: f64,
    knee_db: f64,
    range_db: f64,
    sign: f64,
    offset_db: f64,
    link: f64,
    weight: [f64; 2],
}

impl Law {
    const REST: Law = Law { power: 1.0, attack: 0.0, release: 0.0, threshold_db: 0.0, slope: 0.0, knee_db: 0.0, range_db: 0.0, sign: 1.0, offset_db: 0.0, link: 1.0, weight: [1.0, 1.0] };

    fn of(band: &EqBand, scale: f64, share: f64, sample_rate: f64) -> Law {
        let (hz, q) = clamped(band, sample_rate);
        let spread = (1000.0 / hz).sqrt();
        let attack = (band.attack_ms * 0.001 * spread).max(q / (PI * hz));
        let release = (band.release_ms * 0.001 * spread).max(RELEASE_PERIODS / hz);
        let average = attack.max((AVERAGE_CYCLES * q / hz).min(AVERAGE_LONGEST));
        let hop = HOP as f64;
        Law {
            power: dsp::smoothing(average, sample_rate),
            attack: (-hop / (attack * sample_rate)).exp(),
            release: (-hop / (release * sample_rate)).exp(),
            threshold_db: band.threshold_db,
            slope: slope_of(band.ratio),
            knee_db: band.knee_db,
            range_db: (band.gain_db * scale).abs(),
            sign: if band.gain_db < 0.0 { -1.0 } else { 1.0 },
            offset_db: 10.0 * ((1.0 - share) / share).log10(),
            link: band.link,
            weight: [(1.0 - band.balance).min(1.0), (1.0 + band.balance).min(1.0)],
        }
    }
}

struct Dynamic {
    sample_rate: f64,
    law: Law,
    narrow: Coeffs,
    wide: Coeffs,
    ears: [[Stage; 2]; 2],
    power: [[f64; 2]; 2],
    held: [f64; 2],
    env: [f64; 2],
    run: [Coeffs; 2],
    delta: [Coeffs; 2],
    countdown: usize,
}

impl Dynamic {
    fn new(sample_rate: f64) -> Dynamic {
        Dynamic { sample_rate, law: Law::REST, narrow: Coeffs::FLAT, wide: Coeffs::FLAT, ears: [[Stage::default(); 2]; 2], power: [[0.0; 2]; 2], held: [0.0; 2], env: [0.0; 2], run: [Coeffs::FLAT; 2], delta: [Coeffs::ZERO; 2], countdown: HOP }
    }

    fn retune(&mut self, band: &EqBand, scale: f64) {
        let (hz, q) = clamped(band, self.sample_rate);
        let narrow = svf::band_pass(hz, q, self.sample_rate);
        let wide = svf::band_pass(hz, surround_q(q), self.sample_rate);
        self.law = Law::of(band, scale, noise_gain(&narrow) / noise_gain(&wide), self.sample_rate);
        self.narrow = Coeffs::narrow(narrow);
        self.wide = Coeffs::narrow(wide);
    }

    fn reset(&mut self, band: &EqBand) {
        self.ears = [[Stage::default(); 2]; 2];
        self.power = [[0.0; 2]; 2];
        self.held = [0.0; 2];
        self.env = [0.0; 2];
        self.run = [shaped(band, 0.0, self.sample_rate); 2];
        self.delta = [Coeffs::ZERO; 2];
        self.countdown = HOP;
    }

    fn hear(&mut self, channel: usize, x: f32) {
        let [narrow, wide] = &mut self.ears[channel];
        let n = f64::from(narrow.tick(x, self.narrow));
        let w = f64::from(wide.tick(x, self.wide));
        let [heard_narrow, heard_wide] = &mut self.power[channel];
        *heard_narrow += self.law.power * (n * n - *heard_narrow);
        *heard_wide += self.law.power * (w * w - *heard_wide);
    }

    fn cut_db(&mut self, channel: usize, narrow: f64, wide: f64) -> f64 {
        let law = self.law;
        let over = if wide < SILENCE {
            0.0
        } else {
            let surround = (wide - narrow).max(wide * SURROUND_FLOOR);
            let level_db = 10.0 * ((narrow + POWER_EPS) / surround).log10() + law.offset_db;
            gain_reduction(level_db, law.threshold_db, law.slope, law.knee_db)
        };
        let held = over.max(law.release * self.held[channel] + (1.0 - law.release) * over);
        self.held[channel] = held;
        self.env[channel] = law.attack * self.env[channel] + (1.0 - law.attack) * held;
        self.env[channel].min(law.range_db) * law.weight[channel]
    }

    fn aim(&mut self, band: &EqBand, channels: usize) {
        let mean = [0.5 * (self.power[0][0] + self.power[1][0]), 0.5 * (self.power[0][1] + self.power[1][1])];
        let step = 1.0 / HOP as f32;
        for channel in 0..channels {
            let [mut narrow, mut wide] = self.power[channel];
            if channels == 2 {
                narrow += self.law.link * (mean[0] - narrow);
                wide += self.law.link * (mean[1] - wide);
            }
            let cut = self.cut_db(channel, narrow, wide);
            let target = shaped(band, self.law.sign * cut, self.sample_rate);
            self.delta[channel] = target.zip(self.run[channel], |t, r| (t - r) * step);
        }
        self.countdown = HOP;
    }

    fn advance(&mut self, band: &EqBand, channels: usize) {
        if self.countdown == 0 {
            self.aim(band, channels);
        }
        self.countdown -= 1;
        for (run, delta) in self.run.iter_mut().zip(&self.delta).take(channels) {
            *run = run.zip(*delta, |c, d| c + d);
        }
    }
}

struct Band {
    applied: EqBand,
    run: Coeffs,
    target: Coeffs,
    delta: Coeffs,
    blend: f32,
    blend_delta: f32,
    left: [Stage; PLUS_STAGES],
    right: [Stage; PLUS_STAGES],
    dynamic: Dynamic,
}

impl Band {
    fn new(applied: EqBand, sample_rate: f64) -> Band {
        Band { applied, run: Coeffs::FLAT, target: Coeffs::FLAT, delta: Coeffs::ZERO, blend: 0.0, blend_delta: 0.0, left: [Stage::default(); PLUS_STAGES], right: [Stage::default(); PLUS_STAGES], dynamic: Dynamic::new(sample_rate) }
    }

    fn stages(&self) -> usize {
        stages(self.applied.shape)
    }

    fn clear(&mut self) {
        self.left = [Stage::default(); PLUS_STAGES];
        self.right = [Stage::default(); PLUS_STAGES];
        if self.applied.moving() {
            self.dynamic.reset(&self.applied);
        }
    }

    fn blend_target(&self) -> f32 {
        if self.applied.on { 1.0 } else { 0.0 }
    }

    fn prepare(&mut self, next: EqBand, block: &Block) {
        let reshaped = block.fresh || next.shape != self.applied.shape || next.channel != self.applied.channel || next.moving() != self.applied.moving();
        if block.fresh || block.rescaled || next != self.applied {
            self.target = coefficients(&next, block.scale, block.sample_rate);
            if next.moving() {
                self.dynamic.retune(&next, block.scale);
            }
            self.applied = next;
        }
        if reshaped {
            self.run = self.target;
            self.delta = Coeffs::ZERO;
            self.clear();
        } else {
            let step = 1.0 / block.frames as f32;
            self.delta = self.target.zip(self.run, |t, r| (t - r) * step);
        }
        if block.fresh {
            self.blend = self.blend_target();
            self.blend_delta = 0.0;
        } else {
            self.blend_delta = (self.blend_target() - self.blend) / block.ramp;
        }
    }

    fn tick(&mut self, l: f32, r: f32, tickle: f32) -> (f32, f32) {
        self.run = self.run.zip(self.delta, |c, d| c + d);
        self.blend = (self.blend + self.blend_delta).clamp(0.0, 1.0);
        if self.applied.moving() {
            return self.tick_moving(l, r, tickle);
        }
        let (coeffs, blend, stages) = (self.run, self.blend, self.stages());
        let dry = 1.0 - blend;
        match self.applied.channel {
            Channel::Stereo => {
                let (in_l, in_r) = (l + tickle, r + tickle);
                let out_l = cascade(&mut self.left[..stages], in_l, coeffs);
                let out_r = cascade(&mut self.right[..stages], in_r, coeffs);
                (dry * in_l + blend * out_l, dry * in_r + blend * out_r)
            }
            channel => {
                let mid = 0.5 * (l + r);
                let side = 0.5 * (l - r);
                let (input, keep) = if channel == Channel::Mid { (mid + tickle, side) } else { (side + tickle, mid) };
                let out = cascade(&mut self.left[..stages], input, coeffs);
                let filtered = dry * input + blend * out;
                let (mid, side) = if channel == Channel::Mid { (filtered, keep) } else { (keep, filtered) };
                (mid + side, mid - side)
            }
        }
    }

    fn tick_moving(&mut self, l: f32, r: f32, tickle: f32) -> (f32, f32) {
        let (blend, stages) = (self.blend, self.stages());
        let dry = 1.0 - blend;
        match self.applied.channel {
            Channel::Stereo => {
                let (in_l, in_r) = (l + tickle, r + tickle);
                self.dynamic.hear(0, in_l);
                self.dynamic.hear(1, in_r);
                self.dynamic.advance(&self.applied, 2);
                let out_l = cascade(&mut self.left[..stages], in_l, self.dynamic.run[0]);
                let out_r = cascade(&mut self.right[..stages], in_r, self.dynamic.run[1]);
                (dry * in_l + blend * out_l, dry * in_r + blend * out_r)
            }
            channel => {
                let mid = 0.5 * (l + r);
                let side = 0.5 * (l - r);
                let (input, keep) = if channel == Channel::Mid { (mid + tickle, side) } else { (side + tickle, mid) };
                self.dynamic.hear(0, input);
                self.dynamic.advance(&self.applied, 1);
                let out = cascade(&mut self.left[..stages], input, self.dynamic.run[0]);
                let filtered = dry * input + blend * out;
                let (mid, side) = if channel == Channel::Mid { (filtered, keep) } else { (keep, filtered) };
                (mid + side, mid - side)
            }
        }
    }

    fn settle(&mut self) {
        self.run = self.target;
        self.blend = self.blend_target();
    }

    fn sounds(&self) -> bool {
        self.blend > 0.0 || self.blend_delta > 0.0
    }
}

struct Listen {
    applied: Option<EqBand>,
    heard: bool,
    run: Coeffs,
    target: Coeffs,
    delta: Coeffs,
    blend: f32,
    blend_delta: f32,
    left: [Stage; PLUS_STAGES],
    right: [Stage; PLUS_STAGES],
}

impl Listen {
    fn new() -> Listen {
        Listen { applied: None, heard: false, run: Coeffs::FLAT, target: Coeffs::FLAT, delta: Coeffs::ZERO, blend: 0.0, blend_delta: 0.0, left: [Stage::default(); PLUS_STAGES], right: [Stage::default(); PLUS_STAGES] }
    }

    fn clear(&mut self) {
        self.left = [Stage::default(); PLUS_STAGES];
        self.right = [Stage::default(); PLUS_STAGES];
    }

    fn blend_target(&self) -> f32 {
        if self.heard { 1.0 } else { 0.0 }
    }

    fn prepare(&mut self, next: Option<EqBand>, block: &Block) {
        self.heard = next.is_some();
        if let Some(band) = next {
            let reshaped = block.fresh || self.applied.is_none_or(|applied| applied.shape != band.shape || applied.channel != band.channel);
            if block.fresh || self.applied != Some(band) {
                self.target = Coeffs::narrow(listen(&band, block.sample_rate));
                self.applied = Some(band);
            }
            if reshaped {
                self.run = self.target;
                self.delta = Coeffs::ZERO;
                self.clear();
            } else {
                let step = 1.0 / block.frames as f32;
                self.delta = self.target.zip(self.run, |t, r| (t - r) * step);
            }
        } else {
            self.delta = Coeffs::ZERO;
        }
        if block.fresh {
            self.blend = self.blend_target();
            self.blend_delta = 0.0;
        } else {
            self.blend_delta = (self.blend_target() - self.blend) / block.ramp;
        }
    }

    fn tick(&mut self, dry_l: f32, dry_r: f32, eq_l: f32, eq_r: f32, tickle: f32) -> (f32, f32) {
        let Some(band) = self.applied.filter(|_| self.blend > 0.0 || self.blend_delta > 0.0) else {
            return (eq_l, eq_r);
        };
        self.run = self.run.zip(self.delta, |c, d| c + d);
        self.blend = (self.blend + self.blend_delta).clamp(0.0, 1.0);
        let (coeffs, blend, stages) = (self.run, self.blend, stages(band.shape));
        let (heard_l, heard_r) = match band.channel {
            Channel::Stereo => (cascade(&mut self.left[..stages], dry_l + tickle, coeffs), cascade(&mut self.right[..stages], dry_r + tickle, coeffs)),
            Channel::Mid => {
                let mid = cascade(&mut self.left[..stages], 0.5 * (dry_l + dry_r) + tickle, coeffs);
                (mid, mid)
            }
            Channel::Side => {
                let side = cascade(&mut self.left[..stages], 0.5 * (dry_l - dry_r) + tickle, coeffs);
                (side, -side)
            }
        };
        let keep = 1.0 - blend;
        (keep * eq_l + blend * heard_l, keep * eq_r + blend * heard_r)
    }

    fn settle(&mut self) {
        self.run = self.target;
        self.blend = self.blend_target();
    }

    fn sounds(&self) -> bool {
        self.applied.is_some() && (self.blend > 0.0 || self.blend_delta > 0.0)
    }
}

fn shape(bands: &mut [Band; EQ_BANDS], listen: &mut Listen, l: f32, r: f32, tickle: f32) -> (f32, f32) {
    let (mut x_l, mut x_r) = (l, r);
    for band in bands.iter_mut() {
        (x_l, x_r) = band.tick(x_l, x_r, tickle);
    }
    let (out_l, out_r) = listen.tick(l, r, x_l, x_r, tickle);
    if (out_l * out_l + out_r * out_r).is_finite() {
        return (out_l, out_r);
    }
    for band in bands {
        band.clear();
    }
    listen.clear();
    (0.0, 0.0)
}

pub struct Eq {
    sample_rate: f64,
    bands: [Band; EQ_BANDS],
    listen: Listen,
    pass: BandPass,
    passing: usize,
    scale: f64,
    gain: f32,
    mix: dsp::Mix,
    tickle: f32,
    primed: bool,
    silence: Silence,
}

impl Unit for Eq {
    type Params = params::Eq;

    fn new(sample_rate: f64, _: usize) -> Eq {
        Eq {
            sample_rate,
            bands: params::Eq::DEFAULT.bands.map(|band| Band::new(band, sample_rate)),
            listen: Listen::new(),
            pass: BandPass::new(sample_rate),
            passing: 0,
            scale: 1.0,
            gain: 1.0,
            mix: dsp::Mix::new(sample_rate),
            tickle: TICKLE,
            primed: false,
            silence: Silence::default(),
        }
    }

    fn restart(&mut self) {
        if self.silence.lasting(self.sample_rate) {
            *self = Eq::new(self.sample_rate, 0);
        }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], params: &params::Eq, wet: f32, _: &Clock) {
        let n = l.len().min(r.len());
        let span = ((RAMP_SECONDS * self.sample_rate) as usize).max(1);
        let aim = if params.pass.on { span } else { 0 };
        if !self.primed {
            self.passing = aim;
        }
        let pass = (params.pass.on || self.passing > 0).then(|| self.pass.begin(&params.pass));
        if pass.is_none() {
            self.pass.rest();
        }
        if n == 0 {
            return;
        }
        dsp::sanitize(l, r);
        let quiet = Silence::trailing(l, r);
        let block = Block {
            scale: params.scale,
            sample_rate: self.sample_rate,
            frames: n,
            ramp: span.min(n).max(1) as f32,
            fresh: !self.primed,
            rescaled: params.scale != self.scale,
        };
        for (band, next) in self.bands.iter_mut().zip(params.bands) {
            band.prepare(next, &block);
        }
        self.listen.prepare(params.solo.and_then(|index| params.bands.get(index)).filter(|band| band.on).copied(), &block);
        self.scale = params.scale;
        let gain_target = dsp::db_to_gain(params.trim_db) as f32;
        if block.fresh {
            self.gain = gain_target;
        }
        let gain_delta = if (gain_target - self.gain).abs() < 1e-7 { 0.0 } else { (gain_target - self.gain) / n as f32 };
        let mix_target = wet.clamp(0.0, 1.0);
        self.tickle = -self.tickle;
        let tickle = self.tickle;
        let mut gain = self.gain;
        let mut done = 0;
        if let Some(trim) = pass {
            while done < n && self.passing != aim {
                self.passing = if params.pass.on { self.passing + 1 } else { self.passing - 1 };
                if self.passing == aim {
                    break;
                }
                let share = self.passing as f64 / span as f64;
                let (dry_l, dry_r) = (f64::from(l[done]), f64::from(r[done]));
                let passed = self.pass.tick(dry_l, dry_r, &params.pass, trim);
                let (x_l, x_r) = shape(&mut self.bands, &mut self.listen, (dry_l + share * (passed.gain * passed.l - dry_l)) as f32, (dry_r + share * (passed.gain * passed.r - dry_r)) as f32, tickle);
                gain += gain_delta;
                let mix = f64::from(self.mix.tick(mix_target));
                let wet = mix * f64::from(gain);
                l[done] = ((1.0 - mix) * dry_l + wet * f64::from(x_l)) as f32;
                r[done] = ((1.0 - mix) * dry_r + wet * f64::from(x_r)) as f32;
                done += 1;
            }
        }
        match pass.filter(|_| params.pass.on) {
            None => {
                for i in done..n {
                    let (dry_l, dry_r) = (l[i], r[i]);
                    let (x_l, x_r) = shape(&mut self.bands, &mut self.listen, dry_l, dry_r, tickle);
                    gain += gain_delta;
                    let mix = self.mix.tick(mix_target);
                    let dry = 1.0 - mix;
                    l[i] = dry_l * dry + x_l * gain * mix;
                    r[i] = dry_r * dry + x_r * gain * mix;
                }
            }
            Some(trim) => {
                let shaped = self.bands.iter().any(Band::sounds) || self.listen.sounds();
                for i in done..n {
                    let (dry_l, dry_r) = (f64::from(l[i]), f64::from(r[i]));
                    let mut passed = self.pass.tick(dry_l, dry_r, &params.pass, trim);
                    if shaped {
                        let (x_l, x_r) = shape(&mut self.bands, &mut self.listen, passed.l as f32, passed.r as f32, tickle);
                        (passed.l, passed.r) = (f64::from(x_l), f64::from(x_r));
                    }
                    gain += gain_delta;
                    let mix = f64::from(self.mix.tick(mix_target));
                    let wet = mix * passed.gain * f64::from(gain);
                    l[i] = ((1.0 - mix) * dry_l + wet * passed.l) as f32;
                    r[i] = ((1.0 - mix) * dry_r + wet * passed.r) as f32;
                }
            }
        }
        self.gain = gain_target;
        for band in &mut self.bands {
            band.settle();
        }
        self.listen.settle();
        self.primed = true;
        self.silence.passed(quiet.min(Silence::trailing(l, r)), n);
    }
}

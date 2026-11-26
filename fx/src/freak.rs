use sdk::dsp::crossover::FirSplit;
use sdk::dsp::filter::{Coeffs, Path, State, Tuned};
use sdk::dsp::svf::{Biquad, BiquadCoeffs, StereoBiquad, bell, high_pass, high_pass_6db, high_shelf_q, low_pass, low_shelf_q};
use sdk::dsp::{self, DcCanonical, Greatest, Halfband, Ring, algebraic, algebraic_integral, db_to_gain, finite, follow, lp_coeff, one_pole_lp, settle, sine, smoothing, tanh_integral, triangle, wrap};
use crate::params::freak::{Order, Squeeze, SubSlope};
use crate::params::{self, BandStereo, Curve, DetectMode, DetectSlope, DetectStereo, FREAK_BANDS, FreakSlope, RippleShape};
use crate::compressor::slope_of;
use crate::lfo_shape::{begun, seconds};
use crate::silence::Silence;
use crate::split::{self, DENORMAL, HOP, butterworth_q, cascade, coefficient_bits};
use crate::{Clock, Unit};
use std::f64::consts::{FRAC_1_SQRT_2, FRAC_2_PI, FRAC_PI_2, FRAC_PI_4, PI, TAU};
use std::ops::Mul;

const BANDS: usize = FREAK_BANDS;
const SPLITS: usize = BANDS - 1;
const PASSES: usize = params::Freak::MAX_TIMES;
const CUTS: usize = params::Freak::FILTERS;
const BAND_TONE_STAGES: usize = 4;
const SUB_STAGES: usize = 4;
const SUB_FLOOR_HZ: f64 = 0.1;
const SMOOTH_SECONDS: f64 = 0.030;
const SQUEEZE_SMOOTH_SECONDS: f64 = 0.010;
const PLAIN_FADE_SECONDS: f64 = 0.010;
const BAND_BASS_HZ: f64 = 119.2;
const BAND_BASS_Q: f64 = 0.4665;
const BAND_MID_HZ: f64 = 600.2;
const BAND_MID_Q: f64 = 0.3335;
const BAND_TREBLE_HZ: f64 = 2500.0;
const BAND_TREBLE_Q: f64 = 0.5576;
const BAND_PRESENCE_HZ: f64 = 7000.0;
const BAND_PRESENCE_Q: f64 = FRAC_1_SQRT_2;
const DC_HZ: f64 = 10.0;
const TAPE_PRE_HZ: f64 = 5000.0;
const TAPE_POST_HZ: f64 = 12_000.0;
const TAPE_EMPHASIS: f32 = 0.4;
const ENV_DEPTH_DB: f32 = 12.0;
const FOLD_DRIVE_CAP: f32 = 16.0;
const TUBE_BIAS: f64 = 0.3;
const DIODE_BIAS: f64 = 0.25;
const DIODE_A: f64 = 4.0;
const HARD_CLIP_BIAS: f64 = 0.3;
const RECT_BIAS: f64 = 0.3;
const RECT_HALF_MAKEUP: f32 = 1.83;
const RECT_FULL_MAKEUP: f32 = 2.30;
const ADAA_EPS: f64 = 1e-5;
const LAW_FLOOR: f32 = 1e-6;
const FLAT_TAPS: [f32; 3] = [1.413_43, -0.520_85, 0.107_42];
const SHIFT_SECTIONS: usize = 5;
const SHIFT_I: [f32; SHIFT_SECTIONS] = [-0.018_111_6, -0.327_756_5, -0.328_121_5, 0.831_078, 0.966_951_3];
const SHIFT_J: [f32; SHIFT_SECTIONS] = [-0.252_030_1, -0.252_063_3, 0.568_762_9, 0.924_272_1, 0.991_157_7];
const SHIFT_LAG: usize = 4;
const SHIFT_INDEX: f64 = 1.5 * PI;
const SHIFT_BIAS: f64 = FRAC_PI_4;
const SHIFT_NORMAL: f32 = 3.0;
const SHIFT_FLOOR: f32 = 1e-3;
const SHIFT_TONE_Q: f64 = FRAC_1_SQRT_2;
const SHIFT_CUT_HZ: f64 = 30.0;
const RMS_EPS: f64 = 1e-20;
const GATE_SETTLE_DB: f64 = 1e-3;
const GATE_FADE_DB: f64 = 12.0;
const LOUDEST: f64 = 1e6;
const KNEE_SPAN_FLOOR: f64 = 1e-6;
const PEAK_EPS: f64 = 1e-10;
const SILENCE_DB: f64 = -120.0;
const EXPAND_FLOOR: f64 = 1e-10;
const LEVEL_FLOOR_DB: f64 = -96.0;

fn allpass_chain(x: f32, coefficients: &[f32; SHIFT_SECTIONS], states: &mut [(f32, f32); SHIFT_SECTIONS]) -> f32 {
    coefficients.iter().zip(states).fold(x, |u, (c, (u1, v1))| {
        let v = c * (u + *v1) - *u1;
        *u1 = u;
        *v1 = v;
        v
    })
}

#[derive(Clone, Copy, Default)]
struct Shift {
    lag: [f32; SHIFT_LAG],
    at: usize,
    i: [(f32, f32); SHIFT_SECTIONS],
    j: [(f32, f32); SHIFT_SECTIONS],
    tone: Biquad,
    cut: Biquad,
    up_j: Halfband,
}

impl Shift {
    fn split(&mut self, x: f32) -> (f32, f32) {
        let lagged = self.lag[self.at];
        self.lag[self.at] = x;
        self.at = (self.at + 1) % SHIFT_LAG;
        (-allpass_chain(lagged, &SHIFT_I, &mut self.i), allpass_chain(lagged, &SHIFT_J, &mut self.j))
    }

    fn levelled(i: f32, j: f32, s: &Settings) -> f32 {
        let env = (i * i + j * j + SHIFT_FLOOR * SHIFT_FLOOR).sqrt();
        i * ((1.0 - s.normalize) + s.normalize / (SHIFT_NORMAL * env))
    }

    fn turn(&mut self, i: f32, j: f32, drive: f32, offset: f64, s: &Settings) -> f32 {
        let modulator = self.tone.run(s.shift_tone, f64::from(Shift::levelled(i, j, s))) as f32;
        let (sin, cos) = (offset + SHIFT_INDEX * f64::from(drive * modulator)).sin_cos();
        (f64::from(i) * cos + f64::from(j) * sin) as f32
    }

    fn rest(&mut self, x: f32, s: &Settings) -> (f32, f32) {
        self.lag = [x; SHIFT_LAG];
        let mut u = x;
        for (i, j) in self.i.iter_mut().zip(&mut self.j) {
            (*i, *j) = ((u, -u), (u, -u));
            u = -u;
        }
        let (i, j) = (-u, u);
        self.tone.settle(s.shift_tone, f64::from(Shift::levelled(i, j, s)));
        if s.oversample {
            self.up_j.settle(j);
        }
        (i, j)
    }
}

fn tube(x: f64, b: f64, offset: f64) -> f64 {
    (x + b).tanh() - offset
}

fn tube_integral(x: f64, b: f64, offset: f64) -> f64 {
    tanh_integral(x + b) - offset * x
}

fn diode_leg(u: f64) -> f64 {
    let polarity = if u >= 0.0 { 1.0 } else { -1.0 };
    polarity * (1.0 - (-DIODE_A * u.abs()).exp())
}

fn diode(x: f64, b: f64, offset: f64) -> f64 {
    diode_leg(x + b) - offset
}

fn diode_integral(x: f64, b: f64, offset: f64) -> f64 {
    let a = (x + b).abs();
    a + (-DIODE_A * a).exp_m1() / DIODE_A - offset * x
}

fn soft_leg(u: f64) -> f64 {
    (DIODE_A * u).tanh()
}

fn kneed(x: f64, b: f64, k: f64, offsets: [f64; 2]) -> f64 {
    (1.0 - k) * diode(x, b, offsets[0]) + k * (soft_leg(x + b) - offsets[1])
}

fn kneed_integral(x: f64, b: f64, k: f64, offsets: [f64; 2]) -> f64 {
    (1.0 - k) * diode_integral(x, b, offsets[0]) + k * (tanh_integral(DIODE_A * (x + b)) / DIODE_A - offsets[1] * x)
}

fn op_amp(x: f64) -> f64 {
    if x >= 1.0 {
        1.0
    } else if x <= -1.0 {
        -1.0
    } else {
        1.5 * x - 0.5 * x * x * x
    }
}

fn op_amp_integral(x: f64) -> f64 {
    let a = x.abs();
    if a <= 1.0 {
        let x2 = x * x;
        0.75 * x2 - 0.125 * x2 * x2
    } else {
        a - 0.375
    }
}

fn fold_leg(u: f64) -> f64 {
    (FRAC_PI_2 * u).sin()
}

fn fold(x: f64, b: f64, offset: f64) -> f64 {
    fold_leg(x + b) - offset
}

fn fold_integral(x: f64, b: f64, offset: f64) -> f64 {
    -FRAC_2_PI * (FRAC_PI_2 * (x + b)).cos() - offset * x
}

fn fine_triangle(x: f64, count: f64) -> f64 {
    triangle(0.25 * (count * x - 1.0))
}

fn fine_triangle_integral(x: f64, count: f64) -> f64 {
    let turns = 0.25 * (count * x - 1.0);
    let t = turns - turns.floor() - 0.5;
    4.0 / count * t * (2.0 * t.abs() - 1.0)
}

fn ripple_leg(u: f64, count: f64, shape: RippleShape) -> f64 {
    match shape {
        RippleShape::Sine => (count * FRAC_PI_2 * u).sin(),
        RippleShape::Triangle => fine_triangle(u, count),
    }
}

fn ripple(x: f64, b: f64, share: f64, count: f64, shape: RippleShape, offset: f64) -> f64 {
    share / count * (ripple_leg(x + b, count, shape) - offset)
}

fn ripple_integral(x: f64, b: f64, share: f64, count: f64, shape: RippleShape, offset: f64) -> f64 {
    share / count * match shape {
        RippleShape::Sine => -FRAC_2_PI / count * (count * FRAC_PI_2 * (x + b)).cos() - offset * x,
        RippleShape::Triangle => fine_triangle_integral(x + b, count) - offset * x,
    }
}

fn hard_clip(x: f64, b: f64) -> f64 {
    (x + b).clamp(-1.0, 1.0) - b
}

fn hard_clip_integral(x: f64, b: f64) -> f64 {
    let u = x + b;
    let a = u.abs();
    (if a <= 1.0 { 0.5 * u * u } else { a - 0.5 }) - b * x
}

fn half_rect(x: f64, b: f64) -> f64 {
    (x + b).max(0.0) - b.max(0.0)
}

fn half_rect_integral(x: f64, b: f64) -> f64 {
    let u = (x + b).max(0.0);
    0.5 * u * u - b.max(0.0) * x
}

fn full_rect(x: f64, b: f64) -> f64 {
    (x + b).abs() - b.abs()
}

fn full_rect_integral(x: f64, b: f64) -> f64 {
    let u = x + b;
    0.5 * u * u.abs() - b.abs() * x
}

fn crush(x: f32, drive: f32) -> f32 {
    let bits = (16.0 - 2.0 * drive.max(1.0).log2()).clamp(4.0, 16.0);
    let levels = ((1i32 << (bits + 0.5) as i32) - 1) as f32;
    (x.clamp(-1.0, 1.0) * levels).round() / levels
}

type Bend = ([u32; 3], u64, RippleShape);

#[derive(Clone, Copy, Default)]
struct Adaa {
    x: f64,
    bend: Bend,
    offsets: Option<[f64; 2]>,
    area: Option<f64>,
}

impl Adaa {
    fn bend_to(&mut self, bend: Bend) {
        if self.bend != bend {
            *self = Adaa { x: self.x, bend, ..Adaa::default() };
        }
    }

    fn offsets(&mut self, of: impl FnOnce() -> [f64; 2]) -> [f64; 2] {
        *self.offsets.get_or_insert_with(of)
    }

    fn step(&mut self, x: f32, shape: impl Fn(f64) -> f64, integral: impl Fn(f64) -> f64) -> f32 {
        let x = f64::from(x);
        let dx = x - self.x;
        let y = if dx.abs() < ADAA_EPS {
            self.area = None;
            shape(0.5 * (x + self.x))
        } else {
            let from = self.area.unwrap_or_else(|| integral(self.x));
            let to = integral(x);
            self.area = Some(to);
            (to - from) / dx
        };
        self.x = x;
        y as f32
    }
}

fn shaped(adaa: &mut Adaa, oversample: bool, x: f32, shape: impl Fn(f64) -> f64, integral: impl Fn(f64) -> f64) -> f32 {
    if oversample { adaa.step(x, shape, integral) } else { shape(f64::from(x)) as f32 }
}

#[derive(Clone, Copy, Default)]
struct Poles {
    dc_os: f32,
    dc_base: f32,
    tape_pre: f32,
    tape_post: f32,
    tape_pre_base: f32,
    tape_post_base: f32,
    shift_cut: BiquadCoeffs,
}

#[derive(Clone, Copy)]
struct Settings {
    drive: f32,
    bias: [f32; 2],
    ripples: f32,
    ripple_count: f64,
    ripple_shape: RippleShape,
    dynamics: f32,
    output: f32,
    attack: f32,
    release: f32,
    decimate_inc: f32,
    linked: bool,
    oversample: bool,
    law: Law,
    pre: [f32; 2],
    wet: f32,
    copy: f32,
    flat: bool,
    knee: f32,
    normalize: f32,
    spread: f32,
    shift_tone: BiquadCoeffs,
}

impl Settings {
    fn bend(&self, c: usize) -> Bend {
        ([self.bias[c], self.knee, self.ripples].map(f32::to_bits), self.ripple_count.to_bits(), self.ripple_shape)
    }

    fn lean(&self, c: usize) -> f64 {
        f64::from(self.bias[c]) * SHIFT_BIAS + f64::from(self.spread) * (c as f64 - 0.5)
    }

    fn drive_at(&self, env: f32) -> f32 {
        let swell = if self.dynamics == 0.0 { 1.0 } else { (self.dynamics * env.min(1.0) * ENV_DEPTH_DB / 6.0).exp2() };
        self.drive * swell * self.law.gain(env)
    }
}

#[derive(Clone, Copy)]
struct Law {
    threshold_db: f32,
    lift: f32,
    sharpen: f32,
    range_db: f32,
}

impl Law {
    fn gain(&self, env: f32) -> f32 {
        if self.lift == 0.0 && self.sharpen == 0.0 {
            return 1.0;
        }
        let level_db = 20.0 * env.max(LAW_FLOOR).log10();
        let lifted = (self.lift * (self.threshold_db - level_db).max(0.0)).min(self.range_db);
        let sharpened = (self.sharpen * (level_db - self.threshold_db)).clamp(-self.range_db, self.range_db);
        let db = lifted + sharpened;
        if db == 0.0 {
            return 1.0;
        }
        db_to_gain(f64::from(db)) as f32
    }
}

#[derive(Clone, Copy, Default)]
struct Band {
    poles: Poles,
    env: [f32; 2],
    curve: Option<Curve>,
    up: [Halfband; 2],
    down: [Halfband; 2],
    adaa: [Adaa; 2],
    dc: [DcCanonical; 2],
    tape_pre: [f32; 2],
    tape_post: [f32; 2],
    phase: [Halfband; 2],
    dry: [Halfband; 2],
    spare: Halfband,
    flat: [[f32; 2]; 2],
    base_dc: [DcCanonical; 2],
    held: [f32; 2],
    decimate_phase: Option<f32>,
    tone: [StereoBiquad; BAND_TONE_STAGES],
    shift: [Shift; 2],
}

impl Band {
    fn new(sample_rate: f64) -> Band {
        let os_rate = 2.0 * sample_rate;
        let poles = Poles { dc_os: lp_coeff(DC_HZ, os_rate), dc_base: lp_coeff(DC_HZ, sample_rate), tape_pre: lp_coeff(TAPE_PRE_HZ, os_rate), tape_post: lp_coeff(TAPE_POST_HZ, os_rate), tape_pre_base: lp_coeff(TAPE_PRE_HZ, sample_rate), tape_post_base: lp_coeff(TAPE_POST_HZ, sample_rate), shift_cut: high_pass(SHIFT_CUT_HZ, FRAC_1_SQRT_2, sample_rate) };
        Band { poles, ..Band::default() }
    }

    fn decimate(&mut self, inc: f32) -> bool {
        let Some(phase) = self.decimate_phase else {
            self.decimate_phase = Some(0.0);
            return true;
        };
        let phase = phase + inc;
        let wrapped = phase >= 1.0;
        self.decimate_phase = Some(if wrapped { phase - phase.floor() } else { phase });
        wrapped
    }

    fn prime(&mut self, c: usize, curve: Curve, u: f32, drive: f32, s: &Settings) {
        let bias = f64::from(s.bias[c]);
        let x = f64::from(u);
        let rest = match curve {
            Curve::Soft => x.tanh(),
            Curve::Tube => {
                let b = bias * TUBE_BIAS;
                tube(x, b, b.tanh())
            }
            Curve::Tape => algebraic(x),
            Curve::Diode => {
                let b = bias * DIODE_BIAS;
                kneed(x, b, f64::from(s.knee), [diode_leg(b), soft_leg(b)])
            }
            Curve::OpAmp => op_amp(x),
            Curve::Fold if s.ripples > 0.0 => fold(x, bias, fold_leg(bias)) + ripple(x, bias, f64::from(s.ripples), s.ripple_count, s.ripple_shape, ripple_leg(bias, s.ripple_count, s.ripple_shape)),
            Curve::Fold => fold(x, bias, fold_leg(bias)),
            Curve::HardClip => hard_clip(x, bias * HARD_CLIP_BIAS),
            Curve::HalfRect => f64::from(RECT_HALF_MAKEUP) * half_rect(x, bias * RECT_BIAS),
            Curve::FullRect => f64::from(RECT_FULL_MAKEUP) * full_rect(x, bias * RECT_BIAS),
            Curve::Crush => f64::from(crush(u, drive)),
            Curve::Decimate => x.clamp(-1.0, 1.0),
            Curve::Limit => x / f64::from((self.env[c] * drive).max(1.0)),
            Curve::Phase => {
                let (i, j) = self.shift[c].rest(s.pre[c], s);
                f64::from(self.shift[c].turn(i, j, drive, s.lean(c), s))
            }
        } as f32;
        self.adaa[c] = Adaa { x, ..Adaa::default() };
        self.tape_pre[c] = u;
        self.tape_post[c] = rest;
        self.held[c] = u.clamp(-1.0, 1.0);
        self.dc[c].prime(rest, self.poles.dc_os);
        self.base_dc[c].prime(rest, self.poles.dc_base);
        if s.oversample {
            self.up[c].settle(s.pre[c]);
            self.phase[c].settle(rest);
            self.flat[c] = [rest; 2];
        }
    }

    fn flatten(&mut self, c: usize, y: f32) -> f32 {
        let [z1, z2] = self.flat[c];
        self.flat[c] = [y, z1];
        FLAT_TAPS[0] * y + FLAT_TAPS[1] * z1 + FLAT_TAPS[2] * z2
    }

    fn align(&mut self, c: usize, raw: f32, oversample: bool) -> f32 {
        let aligned = if oversample { self.phase[c].series(raw) } else { raw };
        self.base_dc[c].tick(aligned, self.poles.dc_base)
    }

    fn shape(&mut self, c: usize, curve: Curve, x: f32, s: &Settings) -> f32 {
        let adaa = &mut self.adaa[c];
        adaa.bend_to(s.bend(c));
        let bias = f64::from(s.bias[c]);
        match curve {
            Curve::Soft => shaped(adaa, s.oversample, x, f64::tanh, tanh_integral),
            Curve::Tube => {
                let b = bias * TUBE_BIAS;
                let [offset, _] = adaa.offsets(|| [b.tanh(), 0.0]);
                shaped(adaa, s.oversample, x, |x| tube(x, b, offset), |x| tube_integral(x, b, offset))
            }
            Curve::Tape => {
                let (pre_pole, post_pole) = if s.oversample { (self.poles.tape_pre, self.poles.tape_post) } else { (self.poles.tape_pre_base, self.poles.tape_post_base) };
                let pre = &mut self.tape_pre[c];
                *pre = one_pole_lp(x, pre_pole, *pre);
                let y = shaped(adaa, s.oversample, x + TAPE_EMPHASIS * (x - *pre), algebraic, algebraic_integral);
                let post = &mut self.tape_post[c];
                *post = one_pole_lp(y, post_pole, *post);
                *post
            }
            Curve::Diode => {
                let b = bias * DIODE_BIAS;
                if s.knee > 0.0 {
                    let k = f64::from(s.knee);
                    let offsets = adaa.offsets(|| [diode_leg(b), soft_leg(b)]);
                    shaped(adaa, s.oversample, x, |x| kneed(x, b, k, offsets), |x| kneed_integral(x, b, k, offsets))
                } else {
                    let [offset, _] = adaa.offsets(|| [diode_leg(b), 0.0]);
                    shaped(adaa, s.oversample, x, |x| diode(x, b, offset), |x| diode_integral(x, b, offset))
                }
            }
            Curve::OpAmp => shaped(adaa, s.oversample, x, op_amp, op_amp_integral),
            Curve::Fold if s.ripples > 0.0 => {
                let (share, count, form) = (f64::from(s.ripples), s.ripple_count, s.ripple_shape);
                let [fold_offset, ripple_offset] = adaa.offsets(|| [fold_leg(bias), ripple_leg(bias, count, form)]);
                shaped(adaa, s.oversample, x, |x| fold(x, bias, fold_offset) + ripple(x, bias, share, count, form, ripple_offset), |x| fold_integral(x, bias, fold_offset) + ripple_integral(x, bias, share, count, form, ripple_offset))
            }
            Curve::Fold => {
                let [offset, _] = adaa.offsets(|| [fold_leg(bias), 0.0]);
                shaped(adaa, s.oversample, x, |x| fold(x, bias, offset), |x| fold_integral(x, bias, offset))
            }
            Curve::HardClip => {
                let b = bias * HARD_CLIP_BIAS;
                shaped(adaa, s.oversample, x, |x| hard_clip(x, b), |x| hard_clip_integral(x, b))
            }
            Curve::HalfRect => {
                let b = bias * RECT_BIAS;
                RECT_HALF_MAKEUP * shaped(adaa, s.oversample, x, |x| half_rect(x, b), |x| half_rect_integral(x, b))
            }
            Curve::FullRect => {
                let b = bias * RECT_BIAS;
                RECT_FULL_MAKEUP * shaped(adaa, s.oversample, x, |x| full_rect(x, b), |x| full_rect_integral(x, b))
            }
            Curve::Crush | Curve::Decimate | Curve::Limit | Curve::Phase => x,
        }
    }

    fn process(&mut self, input: [f32; 2], curve: Curve, stereo: BandStereo, s: &Settings, tone: Option<&[BiquadCoeffs; BAND_TONE_STAGES]>) -> [f32; 2] {
        let mid = 0.5 * (input[0] + input[1]);
        let side = 0.5 * (input[0] - input[1]);
        let x = match stereo {
            BandStereo::Stereo => input,
            BandStereo::MidSide => [mid, side],
            BandStereo::Mid => [mid, 0.0],
            BandStereo::Side => [0.0, side],
        };
        let peaks = x.map(f32::abs);
        if s.linked {
            self.env = [follow(self.env[0], peaks[0].max(peaks[1]), s.attack, s.release); 2];
        } else {
            for (env, peak) in self.env.iter_mut().zip(peaks) {
                *env = follow(*env, peak, s.attack, s.release);
            }
        }
        let first = s.drive_at(self.env[0]);
        let drive = [first, if self.env[1].to_bits() == self.env[0].to_bits() { first } else { s.drive_at(self.env[1]) }];
        let fed: [f32; 2] = std::array::from_fn(|c| x[c] + s.pre[c]);
        if self.curve != Some(curve) {
            *self = Band { poles: self.poles, env: self.env, curve: Some(curve), ..Band::default() };
            for c in 0..2 {
                if s.pre[c] != 0.0 {
                    let drive = if curve == Curve::Fold { drive[c].min(FOLD_DRIVE_CAP) } else { drive[c] };
                    self.prime(c, curve, s.pre[c] * drive, drive, s);
                }
            }
        }
        let mut y = [0.0; 2];
        match curve {
            Curve::Phase => {
                for c in 0..2 {
                    let (i, j) = self.shift[c].split(fed[c]);
                    let offset = s.lean(c);
                    let turned = if s.oversample {
                        let (i0, i1) = self.up[c].upsample(i);
                        let (j0, j1) = self.shift[c].up_j.upsample(j);
                        let y0 = self.shift[c].turn(i0, j0, drive[c], offset, s);
                        let y0 = if s.flat { self.flatten(c, y0) } else { y0 };
                        let y1 = self.shift[c].turn(i1, j1, drive[c], offset, s);
                        let y1 = if s.flat { self.flatten(c, y1) } else { y1 };
                        let y0 = self.dc[c].tick(y0, self.poles.dc_os);
                        let y1 = self.dc[c].tick(y1, self.poles.dc_os);
                        self.down[c].downsample(y0, y1)
                    } else {
                        let raw = self.shift[c].turn(i, j, drive[c], offset, s);
                        self.align(c, raw, false)
                    };
                    y[c] = self.shift[c].cut.run(self.poles.shift_cut, f64::from(turned)) as f32;
                }
            }
            Curve::Crush | Curve::Decimate | Curve::Limit => {
                let raw = match curve {
                    Curve::Crush => std::array::from_fn(|c| crush(fed[c] * drive[c], drive[c])),
                    Curve::Limit => std::array::from_fn(|c| fed[c] * drive[c] / (self.env[c] * drive[c]).max(1.0)),
                    _ => {
                        if self.decimate(s.decimate_inc) {
                            self.held = std::array::from_fn(|c| (fed[c] * drive[c]).clamp(-1.0, 1.0));
                        }
                        self.held
                    }
                };
                for c in 0..2 {
                    y[c] = self.align(c, raw[c], s.oversample);
                }
            }
            _ if !s.oversample => {
                let drive = if curve == Curve::Fold { drive.map(|drive| drive.min(FOLD_DRIVE_CAP)) } else { drive };
                for c in 0..2 {
                    let raw = self.shape(c, curve, fed[c] * drive[c], s);
                    y[c] = self.align(c, raw, s.oversample);
                }
            }
            _ => {
                let drive = if curve == Curve::Fold { drive.map(|drive| drive.min(FOLD_DRIVE_CAP)) } else { drive };
                for c in 0..2 {
                    let (u0, u1) = self.up[c].upsample(fed[c]);
                    let y0 = self.shape(c, curve, u0 * drive[c], s);
                    let y0 = if s.flat { self.flatten(c, y0) } else { y0 };
                    let y1 = self.shape(c, curve, u1 * drive[c], s);
                    let y1 = if s.flat { self.flatten(c, y1) } else { y1 };
                    let y0 = self.dc[c].tick(y0, self.poles.dc_os);
                    let y1 = self.dc[c].tick(y1, self.poles.dc_os);
                    y[c] = self.down[c].downsample(y0, y1);
                }
            }
        }
        let shaped = match tone {
            Some(coeffs) => cascade(&mut self.tone, coeffs, y.map(f64::from)).map(|y| y as f32),
            None => y,
        };
        let y: [f32; 2] = std::array::from_fn(|c| {
            let kept = if s.oversample { self.dry[c].series(x[c]) } else { x[c] };
            (shaped[c] * s.wet + kept * s.copy) * s.output
        });
        let half = match stereo {
            BandStereo::Mid => side,
            BandStereo::Side => mid,
            BandStereo::Stereo | BandStereo::MidSide => 0.0,
        };
        let half = if s.oversample { self.spare.series(half) } else { half };
        match stereo {
            BandStereo::Stereo => y,
            BandStereo::MidSide => [y[0] + y[1], y[0] - y[1]],
            BandStereo::Mid => [y[0] + half, y[0] - half],
            BandStereo::Side => [half + y[1], half - y[1]],
        }
    }
}

fn aligned(align: &mut [Halfband; 2], x: [f64; 2], oversample: bool) -> [f64; 2] {
    if oversample { [f64::from(align[0].series(x[0] as f32)), f64::from(align[1].series(x[1] as f32))] } else { x }
}

fn delayed(align: &mut [[Halfband; 2]; PASSES], x: [f64; 2], oversample: bool, passes: usize) -> [f64; 2] {
    align[..passes].iter_mut().fold(x, |x, slot| aligned(slot, x, oversample))
}

#[derive(Clone, Copy)]
enum Signal {
    Wide([f64; 2]),
    Narrow([f32; 2]),
}

impl Signal {
    fn wide(self) -> [f64; 2] {
        match self {
            Signal::Wide(y) => y,
            Signal::Narrow(y) => y.map(f64::from),
        }
    }

    fn narrow(self) -> [f32; 2] {
        match self {
            Signal::Wide(y) => y.map(|y| y as f32),
            Signal::Narrow(y) => y,
        }
    }

    fn plus(self, other: Signal) -> Signal {
        match (self, other) {
            (Signal::Narrow(a), Signal::Narrow(b)) => Signal::Narrow([a[0] + b[0], a[1] + b[1]]),
            (a, b) => {
                let (a, b) = (a.wide(), b.wide());
                Signal::Wide([a[0] + b[0], a[1] + b[1]])
            }
        }
    }
}

trait Sample: Copy + Mul<Output = Self> {
    fn of(x: f64) -> Self;
    fn wide(self) -> f64;
    fn narrow(self) -> f32;
}

impl Sample for f32 {
    fn of(x: f64) -> f32 {
        x as f32
    }

    fn wide(self) -> f64 {
        f64::from(self)
    }

    fn narrow(self) -> f32 {
        self
    }
}

impl Sample for f64 {
    fn of(x: f64) -> f64 {
        x
    }

    fn wide(self) -> f64 {
        self
    }

    fn narrow(self) -> f32 {
        self as f32
    }
}

struct Envelopes {
    follow: f32,
    held: f64,
}

struct Master<'a> {
    params: &'a params::Freak,
    paths: &'a [Path; CUTS],
    cuts: &'a [Coeffs; CUTS],
    trim: f32,
    mix: f32,
    ceiling: f32,
    attack: f32,
    release: f32,
    gains: (f64, f64, f64),
    releasing: bool,
    ceiling_release: f64,
}

fn limited<T: Sample>(x: [T; 2], envelopes: &mut Envelopes, m: &Master) -> [T; 2] {
    if m.params.ceiling_attack_ms > 0.0 {
        let wet = [x[0].narrow(), x[1].narrow()];
        envelopes.follow = follow(envelopes.follow, wet[0].abs().max(wet[1].abs()), m.attack, m.release);
        let held = m.ceiling / envelopes.follow.max(m.ceiling);
        wet.map(|wet| T::of(f64::from((wet * held).clamp(-m.ceiling, m.ceiling))))
    } else {
        let (ceiling, knee, trim) = m.gains;
        let (l, r) = (x[0].wide() * trim, x[1].wide() * trim);
        let gain = if m.releasing {
            let peak = l.abs().max(r.abs());
            envelopes.held = if peak > envelopes.held { peak } else { envelopes.held + m.ceiling_release * (peak - envelopes.held) };
            if envelopes.held > ceiling { ceiling / envelopes.held } else { 1.0 }
        } else {
            1.0
        };
        [T::of(held_under(l * gain, ceiling, knee)), T::of(held_under(r * gain, ceiling, knee))]
    }
}

fn finish<T: Sample>(tone: &mut Tone, envelopes: &mut Envelopes, m: &Master, sum: [T; 2], dry: [f32; 2]) -> [f32; 2] {
    let toned = tone.tick([sum[0].wide(), sum[1].wide()], m).map(T::of);
    let (dry_gain, wet_gain) = if m.params.crossfade {
        let (sin, cos) = (m.mix * std::f32::consts::FRAC_PI_2).sin_cos();
        (cos.max(0.0), sin)
    } else {
        (1.0 - m.mix, m.mix)
    };
    let dry_gain = dry_gain + m.params.dry as f32 * (1.0 - dry_gain);
    let trim = T::of(f64::from(m.trim));
    let mut wet = [toned[0] * trim, toned[1] * trim];
    if m.params.ceiling && !m.params.ceiling_on_mix {
        wet = limited(wet, envelopes, m);
    }
    let mut out = [dry_gain * dry[0] + wet_gain * finite(wet[0].narrow()), dry_gain * dry[1] + wet_gain * finite(wet[1].narrow())];
    if m.params.ceiling && m.params.ceiling_on_mix {
        out = limited(out, envelopes, m);
    }
    out
}

fn butterworth_section_q(order: usize, k: usize) -> f64 {
    0.5 / ((2 * k - 1) as f64 * PI / (2 * order) as f64).sin()
}

fn is_identity(c: &BiquadCoeffs) -> bool {
    coefficient_bits(c) == coefficient_bits(&BiquadCoeffs::default())
}

fn bent(x: f64, ceiling: f64) -> f64 {
    ceiling * (x / ceiling).tanh()
}

fn held_under(x: f64, ceiling: f64, knee: f64) -> f64 {
    let u = x.abs() / ceiling;
    let span = (1.0 - knee).max(KNEE_SPAN_FLOOR);
    let shaped = if u <= knee { u } else { knee + span * ((u - knee) / span).tanh() };
    x.signum() * ceiling * shaped
}

fn expansion(level_db: f64, threshold: f64, slope: f64, knee: f64) -> f64 {
    let under = threshold - level_db;
    if knee > 0.0 && under.abs() <= 0.5 * knee {
        -slope * (under + 0.5 * knee).powi(2) / (2.0 * knee)
    } else if under > 0.0 {
        -slope * under
    } else {
        0.0
    }
}

fn widened(l: f64, r: f64, width: f64) -> (f64, f64) {
    let mid = 0.5 * (l + r);
    let side = 0.5 * (l - r) * width;
    (mid + side, mid - side)
}

fn kept<K: PartialEq, T: Copy>(slot: &mut Option<(K, T)>, key: K, make: impl FnOnce() -> T) -> T {
    match slot {
        Some((kept, value)) if *kept == key => *value,
        _ => {
            let value = make();
            *slot = Some((key, value));
            value
        }
    }
}

fn level_of(peak: f64) -> f32 {
    (20.0 * peak.log10()).clamp(LEVEL_FLOOR_DB, 0.0) as f32
}

struct Crossover {
    sub: [StereoBiquad; SUB_STAGES],
    sub_sections: usize,
    bands: split::Crossover,
    ear_low: [StereoBiquad; SPLITS],
    ear_high: [StereoBiquad; SPLITS],
    recent: [[f64; 2]; 2],
}

impl Crossover {
    fn new(fir: &FirSplit) -> Crossover {
        Crossover { sub: Default::default(), sub_sections: 0, bands: split::Crossover::new(fir), ear_low: Default::default(), ear_high: Default::default(), recent: [[0.0; 2]; 2] }
    }

    fn split(&mut self, x: [f32; 2], tickle: f32, count: usize, f: &Filters, fir: &FirSplit) -> ([[f64; 2]; BANDS], [f32; 2], [f32; 2]) {
        self.recent = [self.recent[1], x.map(f64::from)];
        let (rest, fed) = if self.sub_sections == 0 {
            let fed = [x[0] + tickle, x[1] + tickle];
            (fed.map(f64::from), fed)
        } else if f.sub_wide {
            let rest = cascade(&mut self.sub[..self.sub_sections], &f.sub, [x[0] + tickle, x[1] + tickle].map(f64::from));
            (rest, rest.map(|y| y as f32))
        } else {
            let cut = cascade(&mut self.sub[..self.sub_sections], &f.sub, x.map(f64::from)).map(|y| y as f32);
            let fed = [cut[0] + tickle, cut[1] + tickle];
            (fed.map(f64::from), fed)
        };
        match self.bands.divide(fed, rest, count, &f.split, fir) {
            (bands, Some(delayed)) => (bands, delayed, [delayed[0] + tickle, delayed[1] + tickle]),
            (bands, None) => (bands, fed, fed),
        }
    }

    fn listen(&mut self, x: [f32; 2], count: usize, f: &Filters) -> [[f64; 2]; BANDS] {
        let mut bands = [[0.0f64; 2]; BANDS];
        let mut rest = x.map(f64::from);
        for (s, band) in bands.iter_mut().enumerate().take(count - 1) {
            *band = self.ear_low[s].run(f.ear_low[s], rest);
            rest = self.ear_high[s].run(f.ear_high[s], rest);
        }
        bands[count - 1] = rest;
        bands
    }

    fn idle(&mut self, count: usize) {
        self.bands.idle(count);
        for stage in self.ear_low.iter_mut().skip(count - 1).chain(self.ear_high.iter_mut().skip(count - 1)) {
            stage.reset();
        }
    }

    fn replay_sub(&mut self) {
        for stage in &mut self.sub {
            stage.reset();
            for x in self.recent {
                stage.run(BiquadCoeffs::default(), x);
            }
        }
    }

    fn rest(&mut self) {
        self.bands.rest();
        for stage in self.sub.iter_mut().chain(&mut self.ear_low).chain(&mut self.ear_high) {
            stage.reset();
        }
        self.recent = [[0.0; 2]; 2];
    }
}

fn cut(state: &mut State, x: f64, path: Path, coeffs: &Coeffs) -> f64 {
    let y = state.tick(x as f32, path, coeffs);
    if path == Path::Off { x } else { f64::from(y) }
}

fn approached(mut cuts: [Coeffs; CUTS], target: &[Coeffs; CUTS], a: f32) -> [Coeffs; CUTS] {
    for (mine, theirs) in cuts.iter_mut().zip(target) {
        mine.approach(theirs, a);
    }
    cuts
}

#[derive(Clone, Copy, Default)]
struct Tone {
    cuts: [[State; CUTS]; 2],
}

impl Tone {
    fn tick(&mut self, x: [f64; 2], m: &Master) -> [f64; 2] {
        std::array::from_fn(|c| self.cuts[c].iter_mut().zip(m.paths).zip(m.cuts).fold(x[c], |y, ((state, path), coeffs)| cut(state, y, *path, coeffs)))
    }
}

#[derive(Clone, Copy)]
struct BandLevels {
    input: f32,
    drive: f32,
    dynamics: f32,
    output: f32,
    threshold: f32,
    lift: f32,
    sharpen: f32,
    range: f32,
    wet: f32,
    bias: f32,
    knee: f32,
    ripples: f32,
    decimate_hz: f32,
    normalize: f32,
    spread: f32,
    mix: f32,
}

impl BandLevels {
    fn target(band: &params::freak::FreakBand, punish: f32) -> BandLevels {
        BandLevels {
            input: db_to_gain(band.input_db) as f32,
            drive: db_to_gain(band.drive_db) as f32,
            dynamics: (band.dynamics as f32 + 0.30 * punish).clamp(-1.0, 1.0),
            output: db_to_gain(band.output_db) as f32 * if band.invert { -1.0 } else { 1.0 },
            threshold: band.threshold_db as f32,
            lift: band.lift as f32,
            sharpen: band.sharpen as f32,
            range: band.range_db as f32,
            wet: band.wet as f32,
            bias: band.bias as f32,
            knee: band.knee as f32,
            ripples: band.fold_ripples as f32,
            decimate_hz: band.decimate_hz as f32,
            normalize: band.phase_normalize as f32,
            spread: (band.phase_spread * TAU) as f32,
            mix: band.squeeze.mix as f32,
        }
    }

    fn values(self) -> [f32; 16] {
        let BandLevels { input, drive, dynamics, output, threshold, lift, sharpen, range, wet, bias, knee, ripples, decimate_hz, normalize, spread, mix } = self;
        [input, drive, dynamics, output, threshold, lift, sharpen, range, wet, bias, knee, ripples, decimate_hz, normalize, spread, mix]
    }

    fn toward(&mut self, target: &BandLevels, a: f32) {
        self.input = settle(self.input, target.input, a);
        self.drive = settle(self.drive, target.drive, a);
        self.dynamics = settle(self.dynamics, target.dynamics, a);
        self.output = settle(self.output, target.output, a);
        self.threshold = settle(self.threshold, target.threshold, a);
        self.lift = settle(self.lift, target.lift, a);
        self.sharpen = settle(self.sharpen, target.sharpen, a);
        self.range = settle(self.range, target.range, a);
        self.wet = settle(self.wet, target.wet, a);
        self.bias = settle(self.bias, target.bias, a);
        self.knee = settle(self.knee, target.knee, a);
        self.ripples = settle(self.ripples, target.ripples, a);
        self.decimate_hz = settle(self.decimate_hz, target.decimate_hz, a);
        self.normalize = settle(self.normalize, target.normalize, a);
        self.spread = settle(self.spread, target.spread, a);
        self.mix = settle(self.mix, target.mix, a);
    }
}

#[derive(Clone, Copy)]
struct Levels {
    drive: f32,
    trim: f32,
    ceiling: f32,
    mix: f32,
    bias_lfo_rate: f32,
    bias_lfo_depth: f32,
    bias_lfo_spread: f32,
    bands: [BandLevels; BANDS],
}

impl Levels {
    fn target(params: &params::Freak, wet: f32) -> Levels {
        let punish = params.punish as f32;
        Levels {
            drive: db_to_gain(params.drive_db) as f32 * (1.0 + 4.0 * punish),
            trim: db_to_gain(params.output_db) as f32,
            ceiling: db_to_gain(params.ceiling_db) as f32,
            mix: wet.clamp(0.0, 1.0),
            bias_lfo_rate: params.bias_lfo_rate_hz as f32,
            bias_lfo_depth: params.bias_lfo_depth as f32,
            bias_lfo_spread: params.bias_lfo_spread as f32,
            bands: params.bands.each_ref().map(|band| BandLevels::target(band, punish)),
        }
    }

    fn toward(&mut self, target: &Levels, a: f32) {
        self.drive = settle(self.drive, target.drive, a);
        self.trim = settle(self.trim, target.trim, a);
        self.ceiling = settle(self.ceiling, target.ceiling, a);
        self.mix = settle(self.mix, target.mix, a);
        self.bias_lfo_rate = settle(self.bias_lfo_rate, target.bias_lfo_rate, a);
        self.bias_lfo_depth = settle(self.bias_lfo_depth, target.bias_lfo_depth, a);
        self.bias_lfo_spread = settle(self.bias_lfo_spread, target.bias_lfo_spread, a);
        for (band, t) in self.bands.iter_mut().zip(&target.bands) {
            band.toward(t, a);
        }
    }

    fn still(&self, target: &Levels, a: f32) -> bool {
        let mut next = *self;
        next.toward(target, a);
        next.bits().eq(self.bits())
    }

    fn bits(&self) -> impl Iterator<Item = u32> {
        [self.drive, self.trim, self.ceiling, self.mix, self.bias_lfo_rate, self.bias_lfo_depth, self.bias_lfo_spread].into_iter().chain(self.bands.into_iter().flat_map(BandLevels::values)).map(f32::to_bits)
    }

    fn swing(&self, phase: f64) -> [f32; 2] {
        if self.bias_lfo_depth == 0.0 {
            return [0.0; 2];
        }
        let swung = |turn: f64| self.bias_lfo_depth * sine(phase + turn) as f32;
        [swung(0.0), swung(-0.5 * f64::from(self.bias_lfo_spread))]
    }
}

#[derive(Clone, Copy)]
struct Squeezes {
    threshold: [f64; BANDS],
    slope: [f64; BANDS],
    gain_db: [f64; BANDS],
    up: [f64; BANDS],
    down: [f64; BANDS],
    gate: [f64; BANDS],
    cap: [f64; BANDS],
    ceiling_db: f64,
    knee_db: f64,
    trim_db: f64,
    expand_threshold: [f64; BANDS],
    expand_slope: [f64; BANDS],
    expand_knee: [f64; BANDS],
}

impl Squeezes {
    fn target(p: &params::Freak) -> Squeezes {
        let each = |of: fn(&Squeeze) -> f64| p.bands.each_ref().map(|band| of(&band.squeeze));
        Squeezes {
            threshold: each(|q| q.threshold_db),
            slope: each(|q| slope_of(q.ratio)),
            gain_db: each(|q| q.gain_db),
            up: each(|q| q.up),
            down: each(|q| q.down),
            gate: each(|q| q.gate_floor_db),
            cap: each(|q| q.lift_cap_db),
            ceiling_db: p.ceiling_db,
            knee_db: p.ceiling_knee_db,
            trim_db: p.ceiling_trim_db,
            expand_threshold: each(|q| q.expand_threshold_db),
            expand_slope: each(|q| q.expand_ratio - 1.0),
            expand_knee: each(|q| q.expand_knee_db),
        }
    }

    fn values(&self) -> impl Iterator<Item = f64> {
        self.threshold.into_iter().chain(self.slope).chain(self.gain_db).chain(self.up).chain(self.down).chain(self.gate).chain(self.cap).chain([self.ceiling_db, self.knee_db, self.trim_db]).chain(self.expand_threshold).chain(self.expand_slope).chain(self.expand_knee)
    }

    fn settled(&self, target: &Squeezes, a: f64) -> bool {
        let mut next = *self;
        next.toward(target, a);
        next.values().map(f64::to_bits).eq(self.values().map(f64::to_bits))
    }

    fn ceiling_bits(&self) -> [u64; 3] {
        [self.ceiling_db, self.knee_db, self.trim_db].map(f64::to_bits)
    }

    fn ceiling_gains(&self) -> (f64, f64, f64) {
        (db_to_gain(self.ceiling_db), db_to_gain(-self.knee_db), db_to_gain(self.trim_db))
    }

    fn toward(&mut self, target: &Squeezes, a: f64) {
        let mine = self.threshold.iter_mut().chain(self.slope.iter_mut()).chain(self.gain_db.iter_mut()).chain(self.up.iter_mut()).chain(self.down.iter_mut()).chain(self.gate.iter_mut()).chain(self.cap.iter_mut()).chain([&mut self.ceiling_db, &mut self.knee_db, &mut self.trim_db]).chain(self.expand_threshold.iter_mut()).chain(self.expand_slope.iter_mut()).chain(self.expand_knee.iter_mut());
        for (mine, theirs) in mine.zip(target.values()) {
            *mine += a * (theirs - *mine);
        }
        for (gate, t) in self.gate.iter_mut().zip(target.gate) {
            if (t - *gate).abs() < GATE_SETTLE_DB {
                *gate = t;
            }
        }
    }
}

type Times = ([[u64; 7]; BANDS], [(DetectMode, DetectStereo); BANDS], u64);

#[derive(Clone, Copy)]
struct Timing {
    rms: [f64; BANDS],
    attack: [f64; BANDS],
    release: [f64; BANDS],
    mode: [DetectMode; BANDS],
    stereo: [DetectStereo; BANDS],
    hold: [u64; BANDS],
    peak_release: [f64; BANDS],
    expand_open: [f64; BANDS],
    expand_close: [f64; BANDS],
    ceiling_release: f64,
}

impl Timing {
    fn inputs(p: &params::Freak) -> Times {
        let times = p.bands.each_ref().map(|band| {
            let q = &band.squeeze;
            [q.detect_ms, q.detect_hold_ms, q.detect_release_ms, q.attack_ms, q.release_ms, q.expand_attack_ms, q.expand_release_ms].map(f64::to_bits)
        });
        (times, p.bands.each_ref().map(|band| (band.squeeze.detect_mode, band.squeeze.detect_stereo)), p.ceiling_release_ms.to_bits())
    }

    fn of(p: &params::Freak, sr: f64) -> Timing {
        let rate = |ms: f64| smoothing(ms * 0.001, sr);
        let each = |of: fn(&Squeeze) -> f64| p.bands.each_ref().map(|band| rate(of(&band.squeeze)));
        Timing {
            rms: each(|q| q.detect_ms),
            attack: each(|q| q.attack_ms),
            release: each(|q| q.release_ms),
            mode: p.bands.each_ref().map(|band| band.squeeze.detect_mode),
            stereo: p.bands.each_ref().map(|band| band.squeeze.detect_stereo),
            hold: p.bands.each_ref().map(|band| (band.squeeze.detect_hold_ms * 0.001 * sr).round() as u64),
            peak_release: each(|q| q.detect_release_ms),
            expand_open: each(|q| q.expand_attack_ms),
            expand_close: each(|q| q.expand_release_ms),
            ceiling_release: rate(p.ceiling_release_ms),
        }
    }
}

struct Dynamics {
    mean_square: f64,
    peaks: Greatest,
    held: f64,
    gain_db: f64,
    expand: f64,
}

impl Dynamics {
    fn new(sample_rate: f64) -> Dynamics {
        let room = (Squeeze::MAX_HOLD_MS * 0.001 * sample_rate).ceil() as usize + 1;
        Dynamics { mean_square: 0.0, peaks: Greatest::new(room), held: 0.0, gain_db: 0.0, expand: 1.0 }
    }

    fn rest(&mut self) {
        self.mean_square = 0.0;
        self.peaks.clear();
        self.held = 0.0;
        self.gain_db = 0.0;
        self.expand = 1.0;
    }

    fn tick(&mut self, l: f64, r: f64, band: usize, levels: &Squeezes, timing: &Timing) -> f64 {
        let detected = match timing.stereo[band] {
            DetectStereo::Max => l.abs().max(r.abs()),
            DetectStereo::Mean => (0.5 * (l * l + r * r)).sqrt(),
        }
        .min(LOUDEST);
        let level_db = match timing.mode[band] {
            DetectMode::Rms => {
                self.mean_square = ((1.0 - timing.rms[band]) * self.mean_square + timing.rms[band] * detected * detected).max(RMS_EPS);
                10.0 * self.mean_square.log10()
            }
            DetectMode::Peak => {
                let greatest = self.peaks.greatest(detected, timing.hold[band]);
                self.held = if greatest >= self.held { greatest } else { self.held + timing.peak_release[band] * (greatest - self.held) };
                20.0 * self.held.max(PEAK_EPS).log10()
            }
        };
        let (threshold, slope) = (levels.threshold[band], levels.slope[band]);
        let target = if level_db > threshold {
            -(level_db - threshold) * slope * levels.down[band]
        } else {
            let open = if levels.gate[band] <= Squeeze::GATE_OFF_DB { 1.0 } else { ((level_db - levels.gate[band]) / GATE_FADE_DB).clamp(0.0, 1.0) };
            ((threshold - level_db) * slope * levels.up[band]).min(levels.cap[band]) * open
        };
        let a = if target < self.gain_db { timing.attack[band] } else { timing.release[band] };
        self.gain_db += a * (target - self.gain_db);
        let threshold = levels.expand_threshold[band];
        let expand_db = if threshold <= Squeeze::EXPAND_OFF_DB || level_db <= SILENCE_DB { 0.0 } else { expansion(level_db, threshold, levels.expand_slope[band], levels.expand_knee[band]) };
        let wanted = if expand_db == 0.0 { 1.0 } else { db_to_gain(expand_db) };
        let a = if wanted > self.expand { timing.expand_open[band] } else { timing.expand_close[band] };
        self.expand += a * (wanted - self.expand);
        self.total_db()
    }

    fn total_db(&self) -> f64 {
        if self.expand == 1.0 { self.gain_db } else { self.gain_db + 20.0 * self.expand.max(EXPAND_FLOOR).log10() }
    }
}

#[derive(Clone, Copy)]
struct Press {
    clip: f64,
    width: f64,
    mix: f32,
}

fn squeezed(dynamics: &mut Dynamics, band: usize, x: [f64; 2], heard: [f64; 2], levels: &Squeezes, timing: &Timing, press: Press) -> (f64, [f64; 2]) {
    let gain_db = dynamics.tick(heard[0], heard[1], band, levels, timing);
    let gain = db_to_gain(gain_db + levels.gain_db[band]);
    let (l, r) = (x[0] * gain, x[1] * gain);
    let (l, r) = if press.clip > 0.0 { (bent(l, press.clip), bent(r, press.clip)) } else { (l, r) };
    let (l, r) = if press.width != 1.0 { widened(l, r, press.width) } else { (l, r) };
    let y = if press.mix >= 1.0 {
        [l, r]
    } else if press.mix <= 0.0 {
        x
    } else {
        let m = f64::from(press.mix);
        [x[0] + m * (l - x[0]), x[1] + m * (r - x[1])]
    };
    (gain_db, y)
}

#[derive(Clone, Copy)]
struct Filters {
    sub: [BiquadCoeffs; SUB_STAGES],
    split: split::Filters,
    ear_low: [BiquadCoeffs; SPLITS],
    ear_high: [BiquadCoeffs; SPLITS],
    dry_plain: bool,
    band_tone: [[BiquadCoeffs; BAND_TONE_STAGES]; BANDS],
    shift_tone: [[BiquadCoeffs; 2]; BANDS],
    sub_wide: bool,
    gentle: [bool; BANDS],
}

impl Filters {
    fn target(params: &params::Freak, sr: f64) -> Filters {
        let top = sr * 0.45;
        let split = split::Filters::target(split::Voicing::new(params.crossovers_hz, params.slope, params.count(), params::Freak::CROSSOVER_RATIO), sr);
        let unity = BiquadCoeffs::default();
        let sub = if params.sub_cut_hz <= 0.0 {
            [unity; SUB_STAGES]
        } else {
            let hz = params.sub_cut_hz.clamp(SUB_FLOOR_HZ, top);
            match params.sub_cut_slope {
                SubSlope::Six => [high_pass_6db(hz, sr), unity, unity, unity],
                SubSlope::Twelve => [high_pass(hz, FRAC_1_SQRT_2, sr), unity, unity, unity],
                SubSlope::TwentyFour => [high_pass(hz, butterworth_section_q(4, 1), sr), high_pass(hz, butterworth_section_q(4, 2), sr), unity, unity],
                SubSlope::Thirty => [high_pass_6db(hz, sr), high_pass(hz, butterworth_q(1.0), sr), high_pass(hz, butterworth_q(2.0), sr), unity],
                SubSlope::FortyEight => std::array::from_fn(|k| high_pass(hz, butterworth_section_q(8, k + 1), sr)),
            }
        };
        Filters {
            sub,
            split,
            ear_low: split.hz.map(|hz| low_pass(hz, FRAC_1_SQRT_2, sr)),
            ear_high: split.hz.map(|hz| high_pass(hz, FRAC_1_SQRT_2, sr)),
            dry_plain: params.dry_plain,
            band_tone: params.bands.each_ref().map(|band| [low_shelf_q(band.bass_db, BAND_BASS_HZ, BAND_BASS_Q, sr), bell(BAND_MID_HZ, BAND_MID_Q, band.mid_db, sr), high_shelf_q(band.treble_db, BAND_TREBLE_HZ, BAND_TREBLE_Q, sr), high_shelf_q(band.presence_db, BAND_PRESENCE_HZ, BAND_PRESENCE_Q, sr)]),
            shift_tone: params.bands.each_ref().map(|band| [sr, 2.0 * sr].map(|rate| low_pass(band.phase_tone_hz.clamp(params::Freak::MIN_PHASE_TONE_HZ, top), SHIFT_TONE_Q, rate))),
            sub_wide: params.sub_cut_slope == SubSlope::Thirty,
            gentle: params.bands.each_ref().map(|band| band.squeeze.detect_slope == DetectSlope::Gentle),
        }
    }

    fn toward(&mut self, target: &Filters, a: f64) {
        self.split.toward(&target.split, a);
        self.dry_plain = target.dry_plain;
        self.sub_wide = target.sub_wide;
        self.gentle = target.gentle;
        let ears = self.ear_low.iter_mut().zip(&target.ear_low).chain(self.ear_high.iter_mut().zip(&target.ear_high));
        let tones = self.band_tone.iter_mut().flatten().zip(target.band_tone.iter().flatten()).chain(self.shift_tone.iter_mut().flatten().zip(target.shift_tone.iter().flatten()));
        for (c, t) in self.sub.iter_mut().zip(&target.sub).chain(ears).chain(tones) {
            c.toward(*t, a);
        }
    }

    fn still(&self, target: &Filters, a: f64) -> bool {
        let mut next = *self;
        next.toward(target, a);
        next.bits().eq(self.bits())
    }

    fn bits(&self) -> impl Iterator<Item = u64> {
        let Filters { sub, split, ear_low, ear_high, dry_plain, band_tone, shift_tone, sub_wide, gentle } = self;
        let coefficients = sub.iter().chain(ear_low).chain(ear_high).chain(band_tone.iter().flatten()).chain(shift_tone.iter().flatten());
        split.bits().chain(coefficients.flat_map(coefficient_bits)).chain([u64::from(*dry_plain), u64::from(*sub_wide)]).chain(gentle.iter().map(|gentle| u64::from(*gentle)))
    }
}

#[derive(Clone, Copy, PartialEq)]
struct Voicing {
    crossovers: [u64; SPLITS],
    sub_cut: u64,
    band_tone: [[u64; 4]; BANDS],
    shift_tone: [u64; BANDS],
    choices: (FreakSlope, SubSlope, bool, usize),
    gentle: [bool; BANDS],
}

impl Voicing {
    fn of(params: &params::Freak) -> Voicing {
        Voicing {
            crossovers: params.crossovers_hz.map(f64::to_bits),
            sub_cut: params.sub_cut_hz.to_bits(),
            band_tone: params.bands.each_ref().map(|band| [band.bass_db, band.mid_db, band.treble_db, band.presence_db].map(f64::to_bits)),
            shift_tone: params.bands.each_ref().map(|band| band.phase_tone_hz.to_bits()),
            choices: (params.slope, params.sub_cut_slope, params.dry_plain, params.count()),
            gentle: params.bands.each_ref().map(|band| band.squeeze.detect_slope == DetectSlope::Gentle),
        }
    }
}

pub struct Freak {
    sample_rate: f64,
    smooth: f32,
    squeeze_smooth: f64,
    level: Option<Levels>,
    level_settled_for: Option<Levels>,
    squeezes: Option<Squeezes>,
    designed: Option<(Voicing, Filters)>,
    filters: Option<Filters>,
    filters_settled_for: Option<Voicing>,
    cut_design: [Tuned; CUTS],
    cuts: Option<[Coeffs; CUTS]>,
    timing: Option<(Times, Timing)>,
    gains: Option<([u64; 3], (f64, f64, f64))>,
    count: usize,
    crossover: Crossover,
    fir: FirSplit,
    until_hop: usize,
    held_hz: Option<[f64; SPLITS]>,
    dry: [[Halfband; PASSES]; 2],
    align: [[[Halfband; 2]; PASSES]; BANDS],
    bands: [[Band; PASSES]; BANDS],
    dynamics: [[Dynamics; PASSES]; BANDS],
    tone: Tone,
    plain_fade: Option<usize>,
    line: [Ring; 2],
    tickle: f32,
    bias_phase: Option<f64>,
    ceiling_follow: f32,
    ceiling_env: f64,
    strips: [[f32; 2]; BANDS],
    silence: Silence,
}

impl Freak {
    fn rest(&mut self) {
        (self.level, self.level_settled_for, self.squeezes, self.filters, self.filters_settled_for, self.cuts) = (None, None, None, None, None, None);
        self.crossover.rest();
        (self.until_hop, self.held_hz, self.plain_fade) = (0, None, None);
        self.dry = [[Halfband::default(); PASSES]; 2];
        self.align = [[[Halfband::default(); 2]; PASSES]; BANDS];
        self.bands = [[Band::new(self.sample_rate); PASSES]; BANDS];
        for dynamics in self.dynamics.iter_mut().flatten() {
            dynamics.rest();
        }
        self.tone = Tone::default();
        for line in &mut self.line {
            line.clear();
        }
        (self.tickle, self.ceiling_follow, self.ceiling_env) = (DENORMAL, 0.0, 0.0);
    }
}

impl Unit for Freak {
    type Params = params::Freak;

    fn new(sample_rate: f64, _: usize) -> Freak {
        let fir = FirSplit::new(sample_rate);
        let room = fir.half() + params::HALFBAND_LATENCY * PASSES + 1;
        Freak {
            sample_rate,
            smooth: smoothing(SMOOTH_SECONDS, sample_rate) as f32,
            squeeze_smooth: smoothing(SQUEEZE_SMOOTH_SECONDS, sample_rate),
            level: None,
            level_settled_for: None,
            squeezes: None,
            designed: None,
            filters: None,
            filters_settled_for: None,
            cut_design: params::Freak::DEFAULT.filters().map(|filter| Tuned::of(&filter, sample_rate)),
            cuts: None,
            timing: None,
            gains: None,
            count: 0,
            crossover: Crossover::new(&fir),
            fir,
            until_hop: 0,
            held_hz: None,
            dry: [[Halfband::default(); PASSES]; 2],
            align: [[[Halfband::default(); 2]; PASSES]; BANDS],
            bands: [[Band::new(sample_rate); PASSES]; BANDS],
            dynamics: std::array::from_fn(|_| std::array::from_fn(|_| Dynamics::new(sample_rate))),
            tone: Tone::default(),
            plain_fade: None,
            line: std::array::from_fn(|_| Ring::new(room)),
            tickle: DENORMAL,
            bias_phase: None,
            ceiling_follow: 0.0,
            ceiling_env: 0.0,
            strips: [[0.0; 2]; BANDS],
            silence: Silence::default(),
        }
    }

    fn restart(&mut self) {
        self.bias_phase = None;
        if self.silence.lasting(self.sample_rate) {
            self.rest();
        }
    }

    fn strips(&self) -> [[f32; 2]; BANDS] {
        self.strips
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], params: &params::Freak, wet: f32, clock: &Clock) {
        let n = l.len().min(r.len());
        let (l, r) = (&mut l[..n], &mut r[..n]);
        dsp::sanitize(l, r);
        let quiet = Silence::trailing(l, r);
        let sr = self.sample_rate;
        let count = params.count();
        let delay = params.latency(sr) + 1;
        let engaged = params.engaged();
        let oversample = params.oversample;
        let stereo = params.bands.each_ref().map(|band| band.stereo);
        let active: [bool; BANDS] = std::array::from_fn(|b| b < count && params.bands[b].on);
        let curves: [Option<Curve>; BANDS] = std::array::from_fn(|b| params.bands[b].curve.filter(|_| active[b]));
        let squeezing: [bool; BANDS] = std::array::from_fn(|b| active[b] && params.bands[b].squeezing());
        let times: [usize; BANDS] = std::array::from_fn(|b| params.bands[b].times.clamp(1, PASSES));
        let most = params.most_times();
        for b in 0..BANDS {
            for k in 0..PASSES {
                if curves[b].is_none() || k >= times[b] {
                    self.bands[b][k].curve = None;
                }
                if !squeezing[b] || k >= times[b] {
                    self.dynamics[b][k].rest();
                }
            }
        }
        if self.count != count {
            self.crossover.idle(count);
            self.count = count;
        }
        let attacks: [f32; BANDS] = std::array::from_fn(|b| smoothing(params.bands[b].attack_ms * 0.001, sr) as f32);
        let releases: [f32; BANDS] = std::array::from_fn(|b| smoothing(params.bands[b].release_ms * 0.001, sr) as f32);
        let inv_sr = (1.0 / sr) as f32;
        let smooth = self.smooth;
        let glide = f64::from(smooth);
        let squeeze_smooth = self.squeeze_smooth;
        let target = Levels::target(params, wet);
        let mut level = *self.level.get_or_insert(target);
        let level_moves = !self.level_settled_for.is_some_and(|settled| settled.bits().eq(target.bits())) && !level.still(&target, smooth);
        let squeeze_target = Squeezes::target(params);
        let mut squeezes = self.squeezes.unwrap_or(squeeze_target);
        let squeezes_settled = squeezes.settled(&squeeze_target, squeeze_smooth);
        let timing = kept(&mut self.timing, Timing::inputs(params), || Timing::of(params, sr));
        let voicing = Voicing::of(params);
        let design = kept(&mut self.designed, voicing, || Filters::target(params, sr));
        let mut filters = *self.filters.get_or_insert(design);
        let filters_move = self.filters_settled_for != Some(voicing) && !filters.still(&design, glide);
        self.crossover.sub_sections = (0..SUB_STAGES).rev().find(|&k| !is_identity(&filters.sub[k]) || !is_identity(&design.sub[k])).map_or(0, |k| k + 1);
        let sub_flat = self.crossover.sub_sections == 0;
        for (tuned, filter) in self.cut_design.iter_mut().zip(params.filters()) {
            tuned.tune(&filter, sr);
        }
        let paths = self.cut_design.map(|tuned| tuned.path);
        let cut_target = self.cut_design.map(|tuned| tuned.coeffs);
        let mut cuts = *self.cuts.get_or_insert(cut_target);
        let cuts_move = approached(cuts, &cut_target, smooth) != cuts;
        if let Some(hz) = self.held_hz
            && (filters.split.linear || design.split.linear)
        {
            self.fir.design(&hz[..count - 1]);
        }
        let listening = (0..count).any(|b| squeezing[b] && (filters.gentle[b] || design.gentle[b]));
        let plain_len = ((PLAIN_FADE_SECONDS * sr) as usize).max(1);
        let mut plain_fade = *self.plain_fade.get_or_insert(if engaged { plain_len } else { 0 });
        let mut tickle = self.tickle;
        let mut bias_phase = *self.bias_phase.get_or_insert_with(|| begun(params.bias_lfo_phase, seconds(clock) * f64::from(params.bias_lfo_rate_hz as f32)).1);
        let releasing = params.ceiling_release_ms > 0.0;
        let ceiling_release = timing.ceiling_release;
        let following = params.ceiling_attack_ms > 0.0;
        let ceiling_attack = smoothing(params.ceiling_attack_ms * 0.001, sr) as f32;
        let ceiling_fall = smoothing(params.ceiling_release_ms * 0.001, sr) as f32;
        let mut envelopes = Envelopes { follow: self.ceiling_follow, held: self.ceiling_env };
        let (mut ceiling, mut knee, mut ceiling_trim) = kept(&mut self.gains, squeezes.ceiling_bits(), || squeezes.ceiling_gains());
        let ripple_counts: [f64; BANDS] = std::array::from_fn(|b| params.bands[b].fold_ripple_count.clamp(1, params::Freak::MAX_RIPPLES) as f64);
        let soloing = params.bands[..count].iter().any(|band| band.solo);
        let audible: [bool; BANDS] = std::array::from_fn(|b| if soloing { params.bands[b].solo } else { !params.bands[b].mute });
        let widths = params.bands.each_ref().map(|band| band.squeeze.width);
        let clips = params.bands.each_ref().map(|band| band.squeeze.soft_clip);
        let mut peaks = [0.0f64; BANDS];
        let mut deepest = [0.0f64; BANDS];
        for i in 0..n {
            if self.until_hop == 0 {
                self.held_hz = Some(filters.split.hz);
                if filters.split.linear || design.split.linear {
                    self.fir.design(&filters.split.hz[..count - 1]);
                }
                self.until_hop = HOP;
            }
            self.until_hop -= 1;
            if level_moves {
                level.toward(&target, smooth);
            }
            if !squeezes_settled {
                squeezes.toward(&squeeze_target, squeeze_smooth);
                if params.ceiling {
                    (ceiling, knee, ceiling_trim) = kept(&mut self.gains, squeezes.ceiling_bits(), || squeezes.ceiling_gains());
                }
            }
            if filters_move {
                filters.toward(&design, glide);
            }
            if cuts_move {
                cuts = approached(cuts, &cut_target, smooth);
            }
            let swing = level.swing(bias_phase);
            bias_phase = wrap(bias_phase + f64::from(level.bias_lfo_rate) / sr);
            tickle = -tickle;
            let x = [l[i], r[i]];
            let mut plain = [0.0f32; 2];
            for (line, (slot, x)) in self.line.iter_mut().zip(plain.iter_mut().zip(x)) {
                line.push(x);
                *slot = line.tap(delay);
            }
            let (split, plain_dry, heard) = self.crossover.split(x, tickle, count, &filters, &self.fir);
            let ear = if listening { self.crossover.listen(heard, count, &filters) } else { split };
            let mut wet_sum = Signal::Narrow([0.0; 2]);
            let mut dry_sum = [0.0f64; 2];
            for b in 0..count {
                let band = split[b];
                let matched = if squeezing[b] && widths[b] != 1.0 {
                    let (l, r) = widened(band[0], band[1], widths[b]);
                    [l, r]
                } else {
                    band
                };
                dry_sum[0] += matched[0];
                dry_sum[1] += matched[1];
                let y = if !active[b] {
                    Signal::Wide(delayed(&mut self.align[b], band, oversample, most))
                } else {
                    let p = &params.bands[b];
                    let lv = &level.bands[b];
                    let swung = [lv.bias + swing[0], lv.bias + swing[1]];
                    let (bias, pre) = if p.bias_pre { ([0.0; 2], swung) } else { (swung, [0.0; 2]) };
                    let output = if p.order == Order::Post { 1.0 } else { lv.output };
                    let settings = Settings { drive: level.drive * lv.drive, bias, ripples: lv.ripples, ripple_count: ripple_counts[b], ripple_shape: p.fold_ripple_shape, dynamics: lv.dynamics, output, attack: attacks[b], release: releases[b], decimate_inc: lv.decimate_hz * inv_sr, linked: p.linked, oversample, law: Law { threshold_db: lv.threshold, lift: lv.lift, sharpen: lv.sharpen, range_db: lv.range }, pre, wet: lv.wet, copy: (1.0 - lv.wet) * if p.invert_copy { -1.0 } else { 1.0 }, flat: params.flat_top, knee: lv.knee, normalize: lv.normalize, spread: lv.spread, shift_tone: filters.shift_tone[b][usize::from(oversample)] };
                    let tone = p.toned().then_some(&filters.band_tone[b]);
                    let press = Press { clip: clips[b], width: widths[b], mix: lv.mix };
                    let mut y = Signal::Wide(band);
                    for k in 0..most {
                        let x = y.wide();
                        if k >= times[b] {
                            y = Signal::Wide(aligned(&mut self.align[b][k], x, oversample));
                            continue;
                        }
                        let fed = if curves[b].is_some() && p.order != Order::Pre { Signal::Narrow([(x[0] as f32) * lv.input, (x[1] as f32) * lv.input]) } else { Signal::Wide([x[0] * f64::from(lv.input), x[1] * f64::from(lv.input)]) };
                        let (shaper, align, dynamics) = (&mut self.bands[b][k], &mut self.align[b][k], &mut self.dynamics[b][k]);
                        let shape = |shaper: &mut Band, align: &mut [Halfband; 2], x: Signal| match curves[b] {
                            Some(curve) => Signal::Narrow(shaper.process(x.narrow(), curve, stereo[b], &settings, tone)),
                            None => Signal::Wide(aligned(align, x.wide(), oversample).map(|y| y * f64::from(output))),
                        };
                        y = match p.order {
                            Order::Off => shape(shaper, align, fed),
                            Order::Post => {
                                let shaped = shape(shaper, align, fed).wide();
                                let heard = if filters.gentle[b] { ear[b] } else { shaped };
                                let (gain_db, y) = squeezed(dynamics, b, shaped, heard, &squeezes, &timing, press);
                                deepest[b] = deepest[b].max(-gain_db);
                                Signal::Wide(y.map(|y| y * f64::from(lv.output)))
                            }
                            Order::Pre => {
                                let fed = fed.wide();
                                let heard = if filters.gentle[b] { ear[b] } else { fed };
                                let (gain_db, pressed) = squeezed(dynamics, b, fed, heard, &squeezes, &timing, press);
                                deepest[b] = deepest[b].max(-gain_db);
                                shape(shaper, align, Signal::Wide(pressed))
                            }
                        };
                    }
                    y
                };
                let heard = y.wide();
                peaks[b] = peaks[b].max(heard[0].abs()).max(heard[1].abs());
                if audible[b] {
                    wet_sum = wet_sum.plus(y);
                }
            }
            let dry = if filters.dry_plain { plain_dry } else { [dry_sum[0] as f32, dry_sum[1] as f32] };
            let dry = if oversample { std::array::from_fn(|c| self.dry[c][..most].iter_mut().fold(dry[c], |x, half| half.series(x))) } else { dry };
            let master = Master { params, paths: &paths, cuts: &cuts, trim: level.trim, mix: level.mix, ceiling: level.ceiling, attack: ceiling_attack, release: ceiling_fall, gains: (ceiling, knee, ceiling_trim), releasing, ceiling_release };
            let out = match wet_sum {
                Signal::Narrow(sum) => finish(&mut self.tone, &mut envelopes, &master, sum, dry),
                Signal::Wide(sum) => finish(&mut self.tone, &mut envelopes, &master, sum, dry),
            };
            plain_fade = if engaged { (plain_fade + 1).min(plain_len) } else { plain_fade.saturating_sub(1) };
            let out = if plain_fade >= plain_len {
                out
            } else if plain_fade == 0 {
                plain
            } else {
                let m = plain_fade as f32 / plain_len as f32;
                [plain[0] + m * (out[0] - plain[0]), plain[1] + m * (out[1] - plain[1])]
            };
            l[i] = out[0];
            r[i] = out[1];
        }
        if sub_flat && n > 0 {
            self.crossover.replay_sub();
        }
        self.level = Some(level);
        self.level_settled_for = (!level_moves).then_some(target);
        self.squeezes = Some(squeezes);
        self.filters = Some(filters);
        self.filters_settled_for = (!filters_move).then_some(voicing);
        self.cuts = Some(cuts);
        self.plain_fade = Some(plain_fade);
        self.tickle = tickle;
        self.bias_phase = Some(bias_phase);
        self.ceiling_follow = if params.ceiling && following { envelopes.follow } else { 0.0 };
        self.ceiling_env = if params.ceiling && !following && releasing { envelopes.held } else { 0.0 };
        self.strips = std::array::from_fn(|b| [if squeezing[b] { deepest[b] as f32 } else { 0.0 }, level_of(peaks[b])]);
        self.silence.passed(quiet.min(Silence::trailing(l, r)), n);
    }
}

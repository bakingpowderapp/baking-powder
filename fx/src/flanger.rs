use crate::lfo_shape::{Join, begun, turns};
use crate::{Clock, Unit};
use sdk::dsp::{self, DcBlock, Mix, Ring};
use crate::params::{self, FlangerMode, FlangerShape};
use std::f64::consts::PI;

const RING_SECONDS: f64 = 0.030;
const RING_PAD: usize = 8;
const RING_MIN: usize = 256;
const SMOOTH_SECONDS: f64 = 0.030;
const LFO_START_OFFSET: f64 = 0.25;
const MAX_EXCURSION_MS_STANDARD: f64 = 4.0;
const MAX_EXCURSION_MS_THRU_ZERO: f64 = 10.0;
const MIN_READ_DELAY: f64 = 1.5;
const ALLPASS_INTEGER_EPS: f32 = 1e-4;
const THRU_ZERO_BLEND_SAMPLES: f64 = 2.0;
const NYQUIST_GUARD: f64 = 0.45;
const BBD_STAGES: f64 = 512.0;
const BBD_DIVISOR: f64 = 6.0;
const BBD_MIN_HZ: f64 = 2000.0;
const BBD_MAX_HZ: f64 = 12_000.0;
const TONE_HZ: f64 = 10_000.0;
const TAPE_TILT_HZ: f32 = 6000.0;
const TAPE_TILT: f32 = 0.4;
const TAPE_DRIVE: f32 = 1.6;
const VINTAGE_DRIVE: f32 = 1.3;
const VINTAGE_ASYMMETRY: f32 = 0.05;
const DC_HZ: f64 = 38.293;
const TICKLE: f32 = 1e-25;

#[derive(Clone, Copy)]
struct Smoothed {
    rate: f64,
    center: f64,
    excursion: f64,
    feedback: f32,
    width: f32,
    thru_zero: f32,
    polarity: f32,
}

impl Smoothed {
    fn toward(&mut self, target: &Smoothed, a: f64) {
        let b = a as f32;
        self.rate += a * (target.rate - self.rate);
        self.center += a * (target.center - self.center);
        self.excursion += a * (target.excursion - self.excursion);
        self.feedback += b * (target.feedback - self.feedback);
        self.width += b * (target.width - self.width);
        self.thru_zero += b * (target.thru_zero - self.thru_zero);
        self.polarity += b * (target.polarity - self.polarity);
    }
}

#[derive(Clone, Copy)]
struct Color {
    mode: FlangerMode,
    bbd_coeff: f32,
    tilt_coeff: f32,
    tone_coeff: f32,
    dc_pole: f32,
}

struct Channel {
    ring: Ring,
    allpass_a: f32,
    allpass_b: f32,
    feedback_lp: f32,
    tilt_lp: f32,
    dc: DcBlock,
    blown: bool,
}

impl Channel {
    fn new(samples: usize) -> Channel {
        Channel { ring: Ring::new(samples), allpass_a: 0.0, allpass_b: 0.0, feedback_lp: 0.0, tilt_lp: 0.0, dc: DcBlock::default(), blown: false }
    }

    fn flush(&mut self) {
        self.ring.clear();
        self.allpass_a = 0.0;
        self.allpass_b = 0.0;
        self.feedback_lp = 0.0;
        self.tilt_lp = 0.0;
        self.dc = DcBlock::default();
        self.blown = false;
    }

    fn tick(&mut self, x: f32, delays: (f64, f64), s: &Smoothed, color: Color, tickle: f32) -> f32 {
        let tap_a = allpass_tap(&self.ring, delays.0, &mut self.allpass_a);
        let tap_b = allpass_tap(&self.ring, delays.1, &mut self.allpass_b);
        let flange = tap_a + s.polarity * (s.thru_zero * tap_b);
        let pre = match color.mode {
            FlangerMode::Vintage => {
                self.feedback_lp = dsp::one_pole_lp(x + s.feedback * tap_a, color.bbd_coeff, self.feedback_lp);
                let y = self.feedback_lp;
                self.dc.tick((VINTAGE_DRIVE * (y + VINTAGE_ASYMMETRY * y * y)).tanh() * (1.0 / VINTAGE_DRIVE), color.dc_pole)
            }
            FlangerMode::Tape => {
                let sum = x + s.feedback * tap_a;
                self.tilt_lp = dsp::one_pole_lp(sum, color.tilt_coeff, self.tilt_lp);
                let bright = sum + TAPE_TILT * (sum - self.tilt_lp);
                self.feedback_lp = dsp::one_pole_lp((TAPE_DRIVE * bright).tanh() * (1.0 / TAPE_DRIVE), color.tone_coeff, self.feedback_lp);
                self.dc.tick(self.feedback_lp, color.dc_pole)
            }
            FlangerMode::Standard | FlangerMode::ThruZero => {
                self.feedback_lp = dsp::one_pole_lp(tap_a, color.tone_coeff, self.feedback_lp);
                x + s.feedback * self.dc.tick(self.feedback_lp, color.dc_pole)
            }
        };
        let write = pre + tickle;
        self.blown |= !write.is_finite();
        self.ring.push(dsp::finite(write));
        flange
    }
}

fn allpass_tap(ring: &Ring, delay: f64, state: &mut f32) -> f32 {
    let delay = delay.max(MIN_READ_DELAY);
    let whole = delay as usize;
    let frac = (delay - whole as f64) as f32;
    let newest = ring.tap(whole);
    if frac < ALLPASS_INTEGER_EPS {
        *state = newest;
        return newest;
    }
    let eta = (1.0 - frac) / (1.0 + frac);
    let y = eta * (newest - *state) + ring.tap(whole + 1);
    *state = y;
    y
}

fn lfo(phase: f64, shape: FlangerShape) -> f64 {
    let p = phase + LFO_START_OFFSET;
    let p = p - p.floor();
    match shape {
        FlangerShape::Sine => dsp::sine(phase),
        FlangerShape::Triangle => 1.0 - 2.0 * (2.0 * p - 1.0).abs(),
        FlangerShape::Smoothed | FlangerShape::Log => {
            let u = if p < 0.5 { 2.0 * p } else { 2.0 - 2.0 * p };
            2.0 * (u * u * (3.0 - 2.0 * u)) - 1.0
        }
    }
}

fn delays(s: &Smoothed, lfo: f64, log: bool) -> (f64, f64) {
    if log {
        let swing = ((1.0 + s.excursion / s.center).ln() * lfo).exp();
        (s.center * swing, s.center / swing)
    } else {
        (s.center + s.excursion * lfo, s.center - s.excursion * lfo)
    }
}

pub struct Flanger {
    sample_rate: f64,
    smooth: f64,
    tilt_coeff: f32,
    tone_hz: f64,
    tone_coeff: f32,
    dc_pole: f32,
    left: Channel,
    right: Channel,
    smoothed: Option<Smoothed>,
    phase: Option<f64>,
    join: Join,
    joining: bool,
    mix: Mix,
    tickle: f32,
}

impl Flanger {
    pub(super) fn clear(&mut self) {
        self.left.flush();
        self.right.flush();
        self.smoothed = None;
        self.phase = None;
        self.join = Join::default();
        self.mix = Mix::new(self.sample_rate);
        self.tickle = TICKLE;
    }
}

impl Unit for Flanger {
    type Params = params::Flanger;

    fn new(sample_rate: f64, _: usize) -> Flanger {
        let samples = ((RING_SECONDS * sample_rate).ceil() as usize + RING_PAD).max(RING_MIN);
        let tone_hz = TONE_HZ.min(sample_rate * NYQUIST_GUARD);
        Flanger {
            sample_rate,
            smooth: dsp::smoothing(SMOOTH_SECONDS, sample_rate),
            tilt_coeff: (-2.0 * std::f32::consts::PI * TAPE_TILT_HZ / sample_rate as f32).exp(),
            tone_hz,
            tone_coeff: dsp::lp_coeff(tone_hz, sample_rate),
            dc_pole: dsp::lp_coeff(DC_HZ, sample_rate),
            left: Channel::new(samples),
            right: Channel::new(samples),
            smoothed: None,
            phase: None,
            join: Join::default(),
            joining: false,
            mix: Mix::new(sample_rate),
            tickle: TICKLE,
        }
    }

    fn restart(&mut self) {
        self.joining = true;
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], params: &params::Flanger, wet: f32, clock: &Clock) {
        dsp::sanitize(l, r);
        let sr = self.sample_rate;
        let standard = params.mode == FlangerMode::Standard;
        let center = params.center_ms * 0.001 * sr;
        let reach_ms = if standard { MAX_EXCURSION_MS_STANDARD } else { MAX_EXCURSION_MS_THRU_ZERO };
        let excursion = params.depth * (reach_ms * 0.001 * sr).min(center - MIN_READ_DELAY).max(0.0);
        let blend = (excursion / THRU_ZERO_BLEND_SAMPLES).clamp(0.0, 1.0) as f32;
        let target = Smoothed {
            rate: clock.rate(params.sync, params.division, params.rate_hz),
            center,
            excursion,
            feedback: dsp::shaped_feedback(params.feedback),
            width: params.width as f32,
            thru_zero: if standard { 0.0 } else { blend * blend * (3.0 - 2.0 * blend) },
            polarity: if params.peak { 1.0 } else { -1.0 },
        };
        let (mode, shape, spread) = (params.mode, params.shape, params.spread);
        let log = shape == FlangerShape::Log;
        let bbd_track = BBD_STAGES * sr / BBD_DIVISOR;
        let (smooth, tilt_coeff, tone_hz, tone_coeff, dc_pole) = (self.smooth, self.tilt_coeff, self.tone_hz, self.tone_coeff, self.dc_pole);
        let wet = wet.clamp(0.0, 1.0);
        let mut s = self.smoothed.unwrap_or(target);
        let mut phase = match self.phase {
            Some(phase) if !self.joining => phase,
            was => {
                let now = begun(params.start_phase, turns(clock, params.sync, params.division, params.rate_hz)).1;
                if let Some(was) = was {
                    self.join.begin(was, now, sr);
                }
                now
            }
        };
        self.joining = false;
        let mut tickle = self.tickle;
        let mut blown = false;
        for i in 0..l.len().min(r.len()) {
            tickle = -tickle;
            s.toward(&target, smooth);
            let swept = self.join.swept(phase);
            let phase_r = dsp::wrap(swept + 0.5 * spread);
            let (lfo_l, lfo_r) = (lfo(swept, shape), lfo(phase_r, shape));
            phase = dsp::wrap(phase + s.rate / sr);
            let bbd_coeff = if mode == FlangerMode::Vintage {
                let hz = (bbd_track / s.center).clamp(BBD_MIN_HZ, BBD_MAX_HZ).min(tone_hz);
                ((-2.0 * PI * hz / sr) as f32).exp()
            } else {
                0.0
            };
            let color = Color { mode, bbd_coeff, tilt_coeff, tone_coeff, dc_pole };
            let mut flange_l = self.left.tick(l[i], delays(&s, lfo_l, log), &s, color, tickle);
            let mut flange_r = self.right.tick(r[i], delays(&s, lfo_r, log), &s, color, tickle);
            if (s.width - 1.0).abs() > dsp::WIDTH_UNITY_EPS {
                (flange_l, flange_r) = dsp::widen(flange_l, flange_r, s.width);
            }
            let mix = self.mix.tick(wet);
            let dry = 1.0 - mix + params.dry as f32 * mix;
            let out_l = dry * l[i] + mix * flange_l;
            let out_r = dry * r[i] + mix * flange_r;
            blown |= !out_l.is_finite() || !out_r.is_finite();
            l[i] = dsp::finite(out_l);
            r[i] = dsp::finite(out_r);
        }
        if blown || self.left.blown || self.right.blown {
            self.left.flush();
            self.right.flush();
        }
        self.smoothed = Some(s);
        self.phase = Some(phase);
        self.tickle = tickle;
    }
}

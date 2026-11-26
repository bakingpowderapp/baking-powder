use sdk::dsp::svf::{Biquad, high_pass};
use sdk::dsp::{self, Greatest, Halfband, Mix, Ring};
use crate::compressor::{ring_frames, sounding};
use crate::{Clock, Unit, params};
use std::f64::consts::{FRAC_1_SQRT_2, LOG2_10, PI};

const WIDTH_EPSILON: f32 = 0.001;
const SIDE_HP_HZ: f64 = 80.0;
const DB_TO_EXP2: f32 = (LOG2_10 / 20.0) as f32;
const MAX_LOOKAHEAD_SECONDS: f64 = 0.010;
const TAIL_SNAP_DB: f32 = 1e-4;
const SWAP_SNAP: f32 = 1e-4;
const POWER_SECONDS: f64 = 0.200;
const POWER_FLOOR: f32 = 1e-10;
const COMPENSATION_SECONDS: f64 = 0.010;

#[derive(Clone, Copy)]
struct Release {
    step: f32,
    hold: u64,
    tail_pole: f32,
    tail_share: f32,
}

impl Release {
    fn of(p: &params::Utility, sample_rate: f64) -> Release {
        Release {
            step: (p.release_db_per_s / sample_rate) as f32,
            hold: (p.hold_ms * 0.001 * sample_rate).round() as u64,
            tail_pole: (-1.0 / (p.tail_ms * 0.001 * sample_rate)).exp() as f32,
            tail_share: p.tail_share as f32,
        }
    }
}

struct Limiter {
    lookahead: usize,
    smooth: bool,
    true_peak: bool,
    weights: Vec<f32>,
    up: [[Halfband; 2]; 2],
    wet_l: Ring,
    wet_r: Ring,
    env: Ring,
    dry_l: Ring,
    dry_r: Ring,
    hold: Greatest,
    moving_sum: f32,
    env_db: f32,
    tail_db: f32,
    idle: usize,
    on: bool,
    reduced_db: f32,
}

impl Limiter {
    fn new(sample_rate: f64, max_frames: usize) -> Limiter {
        let longest = (MAX_LOOKAHEAD_SECONDS * sample_rate).ceil() as usize + 1;
        let ring = || Ring::new(max_frames + longest + 2);
        let hold = Greatest::new((params::Utility::MAX_HOLD_MS * 0.001 * sample_rate).ceil() as usize + 1);
        Limiter { lookahead: 1, smooth: false, true_peak: false, weights: Vec::with_capacity(longest), up: [[Halfband::default(); 2]; 2], wet_l: ring(), wet_r: ring(), env: ring(), dry_l: ring(), dry_r: ring(), hold, moving_sum: 0.0, env_db: 0.0, tail_db: 0.0, idle: 1, on: false, reduced_db: 0.0 }
    }

    fn set(&mut self, lookahead: usize, smooth: bool, true_peak: bool) {
        if true_peak != self.true_peak {
            self.true_peak = true_peak;
            self.up = [[Halfband::default(); 2]; 2];
        }
        if lookahead == self.lookahead && smooth == self.smooth {
            return;
        }
        self.lookahead = lookahead;
        self.smooth = smooth;
        let rise = |k: usize| (PI * k as f64 / (lookahead + 1) as f64).sin().powi(2);
        let total: f64 = (1..=lookahead).map(rise).sum();
        self.weights.clear();
        self.weights.extend((1..=lookahead).map(|k| (rise(k) / total) as f32));
        self.moving_sum = (1..=lookahead).map(|k| self.env.tap(k)).sum();
        self.idle = self.idle.min(lookahead);
    }

    fn arm(&mut self, on: bool) {
        if on && !self.on {
            self.up = [[Halfband::default(); 2]; 2];
            self.env.clear();
            self.hold.clear();
            self.moving_sum = 0.0;
            self.env_db = 0.0;
            self.tail_db = 0.0;
            self.idle = self.lookahead;
        }
        self.on = on;
    }

    fn limit(&mut self, x_l: f32, x_r: f32, ceiling_db: f32, ceiling: f32, release: Release) -> (f32, f32) {
        let lookahead = self.lookahead;
        self.wet_l.push(x_l);
        self.wet_r.push(x_r);
        let peak = if self.true_peak { self.between(x_l, x_r) } else { x_l.abs().max(x_r.abs()) };
        let wanted = if peak > ceiling { (ceiling_db - 20.0 * peak.log10()).min(0.0) } else { 0.0 };
        let held = -(self.hold.greatest(f64::from(-wanted), release.hold) as f32);
        self.env_db = held.min(self.env_db + release.step);
        self.tail_db = (self.tail_db * release.tail_pole).min(release.tail_share * held);
        if self.tail_db > -TAIL_SNAP_DB {
            self.tail_db = 0.0;
        }
        let env_db = self.env_db.min(self.tail_db);
        let leaving = self.env.tap(lookahead);
        self.env.push(env_db);
        self.moving_sum += env_db - leaving;
        if env_db == 0.0 {
            self.idle = (self.idle + 1).min(lookahead);
            if self.idle >= lookahead {
                self.moving_sum = 0.0;
            }
        } else {
            self.idle = 0;
        }
        let (gain, reduced_db) = if self.idle >= lookahead {
            (1.0, 0.0)
        } else {
            let reduction_db = if self.smooth { self.weights.iter().enumerate().map(|(k, w)| w * self.env.tap(k + 1)).sum::<f32>() } else { self.moving_sum / lookahead as f32 };
            (10f32.powf(reduction_db / 20.0), -reduction_db)
        };
        self.reduced_db = reduced_db;
        let limited = |x: f32| (x * gain).clamp(-ceiling, ceiling);
        (limited(self.wet_l.tap(lookahead)), limited(self.wet_r.tap(lookahead)))
    }

    fn between(&mut self, x_l: f32, x_r: f32) -> f32 {
        let mut peak = x_l.abs().max(x_r.abs());
        for (up, x) in self.up.iter_mut().zip([x_l, x_r]) {
            let [first, second] = up;
            let (a, b) = first.upsample(x);
            for half in [a, b] {
                let (c, d) = second.upsample(half);
                peak = peak.max(c.abs()).max(d.abs());
            }
        }
        peak
    }

    fn delay_dry(&mut self, d_l: f32, d_r: f32) -> (f32, f32) {
        self.dry_l.push(d_l);
        self.dry_r.push(d_r);
        (self.dry_l.tap(self.lookahead), self.dry_r.tap(self.lookahead))
    }

    fn advance(&mut self, x_l: f32, x_r: f32, d_l: f32, d_r: f32) {
        self.wet_l.push(x_l);
        self.wet_r.push(x_r);
        self.dry_l.push(d_l);
        self.dry_r.push(d_r);
    }
}

pub struct Utility {
    sample_rate: f64,
    knob_coeff: f32,
    power_coeff: f32,
    compensation_coeff: f32,
    gain_db: f32,
    balance: f32,
    swap: f32,
    mix: Mix,
    dry_l: Vec<f32>,
    dry_r: Vec<f32>,
    side_hp: Biquad,
    width: f32,
    width_idle: bool,
    weighed: f32,
    power_m: f32,
    power_s: f32,
    compensation: f32,
    limiter: Limiter,
    primed: bool,
    gr_meter: f32,
}

impl Unit for Utility {
    type Params = params::Utility;

    fn gauge(&self) -> f32 {
        self.gr_meter
    }

    fn new(sample_rate: f64, max_frames: usize) -> Utility {
        let frames = max_frames.max(1);
        Utility {
            sample_rate,
            knob_coeff: dsp::smoothing(0.005, sample_rate) as f32,
            power_coeff: dsp::smoothing(POWER_SECONDS, sample_rate) as f32,
            compensation_coeff: dsp::smoothing(COMPENSATION_SECONDS, sample_rate) as f32,
            gain_db: 0.0,
            balance: 0.0,
            swap: 0.0,
            mix: Mix::new(sample_rate),
            dry_l: vec![0.0; frames],
            dry_r: vec![0.0; frames],
            side_hp: Biquad::with(high_pass(SIDE_HP_HZ, FRAC_1_SQRT_2, sample_rate)),
            width: 1.0,
            width_idle: true,
            weighed: 0.0,
            power_m: 0.0,
            power_s: 0.0,
            compensation: 1.0,
            limiter: Limiter::new(sample_rate, frames),
            primed: false,
            gr_meter: 0.0,
        }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], p: &params::Utility, wet: f32, _: &Clock) {
        let n = l.len().min(r.len()).min(self.dry_l.len());
        if n == 0 {
            return;
        }
        let (l, r) = (&mut l[..n], &mut r[..n]);
        dsp::sanitize(l, r);
        if !self.primed {
            self.gain_db = p.gain_db as f32;
            self.balance = p.balance as f32;
            self.swap = f32::from(u8::from(p.swap));
            self.primed = true;
        }
        self.dry_l[..n].copy_from_slice(l);
        self.dry_r[..n].copy_from_slice(r);
        if p.swap || self.swap > 0.0 {
            self.swap(l, r, p.swap);
        }
        let width_target = p.width as f32;
        if (width_target - 1.0).abs() >= WIDTH_EPSILON {
            self.widen(l, r, width_target, p.plain);
        } else {
            self.narrow();
        }
        self.finish(l, r, p, wet);
    }

    fn restart(&mut self) {
        let queued = self.limiter.lookahead.max(ring_frames(self.sample_rate));
        if !sounding(&self.limiter.dry_l, &self.limiter.dry_r, queued) && !sounding(&self.limiter.wet_l, &self.limiter.wet_r, queued) {
            self.narrow();
            self.limiter.on = false;
            self.mix = Mix::new(self.sample_rate);
            self.primed = false;
        }
    }
}

impl Utility {
    fn narrow(&mut self) {
        self.side_hp.reset();
        self.width_idle = true;
        self.weighed = 0.0;
        self.power_m = 0.0;
        self.power_s = 0.0;
        self.compensation = 1.0;
    }

    fn swap(&mut self, l: &mut [f32], r: &mut [f32], on: bool) {
        let target = if on { 1.0 } else { 0.0 };
        let coeff = self.knob_coeff;
        let mut s = self.swap;
        for i in 0..l.len() {
            s += coeff * (target - s);
            if (target - s).abs() < SWAP_SNAP {
                s = target;
            }
            let (x_l, x_r) = (l[i], r[i]);
            l[i] = x_l + s * (x_r - x_l);
            r[i] = x_r + s * (x_l - x_r);
        }
        self.swap = s;
    }

    fn widen(&mut self, l: &mut [f32], r: &mut [f32], target: f32, plain: bool) {
        let (knob_coeff, power_coeff, compensation_coeff) = (self.knob_coeff, self.power_coeff, self.compensation_coeff);
        let (mut idle, mut width) = (self.width_idle, self.width);
        let (mut weighed, mut power_m, mut power_s, mut compensation) = (self.weighed, self.power_m, self.power_s, self.compensation);
        for i in 0..l.len() {
            width = if idle { target } else { width + knob_coeff * (target - width) };
            idle = false;
            let mid = (l[i] + r[i]) * 0.5;
            if plain {
                let side = (l[i] - r[i]) * 0.5 * width;
                l[i] = mid + side;
                r[i] = mid - side;
                continue;
            }
            let side = self.side_hp.tick(f64::from((l[i] - r[i]) * 0.5)) as f32;
            weighed += 1.0;
            let fresh = 1.0 / weighed;
            let share = power_coeff.max(fresh);
            power_m += share * (mid * mid + POWER_FLOOR - power_m);
            power_s += share * (side * side + POWER_FLOOR - power_s);
            let level = ((power_m + power_s) / (power_m + width * width * power_s)).sqrt().clamp(0.5, 1.25);
            compensation += compensation_coeff.max(fresh) * (level - compensation);
            let side = side * width;
            l[i] = (mid + side) * compensation;
            r[i] = (mid - side) * compensation;
        }
        self.width_idle = idle;
        self.width = width;
        self.weighed = weighed;
        self.power_m = power_m;
        self.power_s = power_s;
        self.compensation = compensation;
    }

    fn finish(&mut self, l: &mut [f32], r: &mut [f32], p: &params::Utility, wet: f32) {
        let (gain_target, balance_target) = (p.gain_db as f32, p.balance as f32);
        let ceiling_db = p.ceiling_db as f32;
        let ceiling = dsp::db_to_gain(p.ceiling_db) as f32;
        let release = Release::of(p, self.sample_rate);
        let hard = p.limit && p.hard;
        let limiting = p.limit && !hard;
        self.limiter.set(p.lookahead_samples(self.sample_rate), p.lookahead_curve == params::LookaheadCurve::Smooth, p.true_peak);
        self.limiter.arm(limiting);
        let knob_coeff = self.knob_coeff;
        let (mut gain_db, mut balance, mut deepest) = (self.gain_db, self.balance, 0.0f32);
        for i in 0..l.len() {
            gain_db += knob_coeff * (gain_target - gain_db);
            balance += knob_coeff * (balance_target - balance);
            let mix = self.mix.tick(wet);
            let gain = (gain_db * DB_TO_EXP2).exp2();
            let mut v_l = l[i] * gain * (1.0 - balance.max(0.0));
            let mut v_r = r[i] * gain * (1.0 + balance.min(0.0));
            let (mut d_l, mut d_r) = (self.dry_l[i], self.dry_r[i]);
            if limiting {
                (v_l, v_r) = self.limiter.limit(v_l, v_r, ceiling_db, ceiling, release);
                (d_l, d_r) = self.limiter.delay_dry(d_l, d_r);
                deepest = deepest.max(self.limiter.reduced_db);
            } else {
                self.limiter.advance(v_l, v_r, d_l, d_r);
                if hard {
                    let peak = v_l.abs().max(v_r.abs());
                    if peak > ceiling {
                        deepest = deepest.max(20.0 * (peak / ceiling).log10());
                    }
                    v_l = v_l.clamp(-ceiling, ceiling);
                    v_r = v_r.clamp(-ceiling, ceiling);
                }
            }
            l[i] = d_l + mix * (v_l - d_l);
            r[i] = d_r + mix * (v_r - d_r);
        }
        self.gain_db = gain_db;
        self.balance = balance;
        self.gr_meter = deepest;
    }
}

use sdk::dsp::{self, Mix, settle};
use crate::params;
use crate::{Clock, Unit};
use std::f64::consts::{LN_10, TAU};

const DETECTOR_LOW_CUT_HZ: f64 = 70.0;
const DB_PER_NEPER: f64 = 20.0 / LN_10;
const QUIET: f64 = 1e-30;
const STILL_POWER: f64 = 1e-9;
const KNOB_SMOOTH_S: f64 = 0.03;

const ATTACK_KNOB_STRAIGHT: f64 = 0.6;
const ATTACK_KNOB_POWER: i32 = 5;

const ATTACK_SILENCE_DB: f64 = -100.0;
const ATTACK_POWER: f64 = 1.9;
const ATTACK_FAST_RISE_S: f64 = 0.002;
const ATTACK_FAST_FALL_S: f64 = 0.004;
const ATTACK_SLOW_RISE_S: f64 = 0.014;
const ATTACK_SLOW_FALL_S: f64 = 0.0038;
const ATTACK_SLOW_BLEND: f64 = 0.3;
const ATTACK_EDGE_SLOPE: f64 = 10.2;
const ATTACK_EDGE_TOP_DB: f64 = 7.6;
const ATTACK_EDGE_KNEE_DB: f64 = 0.4;
const ATTACK_GAP_SLOPE: f64 = 0.74;
const ATTACK_LEVEL_SLOPE: f64 = 0.48;
const ATTACK_BIAS_DB: f64 = 63.8;
const ATTACK_TOP_DB: f64 = 18.0;
const ATTACK_TOP_KNEE_DB: f64 = 0.9;
const ATTACK_LIFT_RISE_S: f64 = 0.000_08;
const ATTACK_LIFT_FALL_S: f64 = 0.015;

const SUSTAIN_SILENCE_DB: f64 = -78.2;
const SUSTAIN_ZERO_DB: f64 = -109.3;
const SUSTAIN_QUICK_KNEE_DB: f64 = 5.6;
const SUSTAIN_QUICK_CHARGE_S: f64 = 10.9;
const SUSTAIN_QUICK_LEAK_S: f64 = 0.0702;
const SUSTAIN_QUICK_SLEW_DB_PER_S: f64 = 12_000.0;
const SUSTAIN_QUICK_REST_DB: f64 = 4.28;
const SUSTAIN_LONG_KNEE_DB: f64 = 5.4;
const SUSTAIN_LONG_CHARGE_S: f64 = 63.0;
const SUSTAIN_LONG_LEAK_S: f64 = 1.1;
const SUSTAIN_LONG_REST_DB: f64 = 7.44;
const SUSTAIN_JUMP_EDGE_DB: f64 = -2.3;
const SUSTAIN_JUMP_SLOPE: f64 = 9.3;
const SUSTAIN_JUMP_SHARE: f64 = 0.454;
const SUSTAIN_SLOPE: f64 = 0.94;
const SUSTAIN_OFFSET_DB: f64 = -2.6;
const SUSTAIN_FLOOR_DB: f64 = -9.5;
const SUSTAIN_CEILING_DB: f64 = 36.0;

const CLIP_KNEE: f32 = 0.9;

fn attack_share(shown_db: f64) -> f64 {
    let turn = (shown_db.abs() / params::Transient::ATTACK_DB).min(1.0);
    (ATTACK_KNOB_STRAIGHT * turn + (1.0 - ATTACK_KNOB_STRAIGHT) * turn.powi(ATTACK_KNOB_POWER)).copysign(shown_db)
}

fn decibels(amplitude: f64) -> f64 {
    amplitude.ln() * DB_PER_NEPER
}

fn amplitude(decibels: f64) -> f64 {
    (decibels / DB_PER_NEPER).exp()
}

fn clip(x: f32) -> f32 {
    let level = x.abs();
    if level <= CLIP_KNEE {
        return x;
    }
    let span = 1.0 - CLIP_KNEE;
    let over = (level - CLIP_KNEE) / span;
    (1.0 - span / (1.0 + over + over * over)).copysign(x)
}

#[derive(Clone, Copy)]
struct Knobs {
    attack: f32,
    sustain: f32,
    clip: f32,
    out_gain: f32,
}

impl Knobs {
    fn of(p: &params::Transient) -> Knobs {
        Knobs {
            attack: attack_share(p.attack_db) as f32,
            sustain: (p.sustain_db / params::Transient::SUSTAIN_DB).clamp(-1.0, 1.0) as f32,
            clip: if p.clip { 1.0 } else { 0.0 },
            out_gain: dsp::db_to_gain(p.output_db) as f32,
        }
    }

    fn settle(&mut self, target: &Knobs, a: f32) {
        self.attack = settle(self.attack, target.attack, a);
        self.sustain = settle(self.sustain, target.sustain, a);
        self.clip = settle(self.clip, target.clip, a);
        self.out_gain = settle(self.out_gain, target.out_gain, a);
    }
}

#[derive(Clone, Copy)]
struct Detector {
    held: [f64; 2],
    last: [f64; 2],
}

impl Detector {
    const SILENT: Detector = Detector { held: [0.0; 2], last: [0.0; 2] };

    fn level(&mut self, l: f64, r: f64, keep: f64) -> f64 {
        self.held = [l - self.last[0] + keep * self.held[0], r - self.last[1] + keep * self.held[1]];
        self.last = [l, r];
        for held in &mut self.held {
            if held.abs() < QUIET {
                *held = 0.0;
            }
        }
        self.held[0].abs().max(self.held[1].abs())
    }
}

struct AttackRates {
    fast_rise: f64,
    fast_fall: f64,
    slow_rise: f64,
    slow_fall: f64,
    lift_rise: f64,
    lift_keep: f64,
    floor: f64,
}

impl AttackRates {
    fn at(step: f64) -> AttackRates {
        let passing = |seconds: f64| 1.0 - (-step / seconds).exp();
        AttackRates {
            fast_rise: passing(ATTACK_FAST_RISE_S),
            fast_fall: passing(ATTACK_FAST_FALL_S),
            slow_rise: passing(ATTACK_SLOW_RISE_S),
            slow_fall: passing(ATTACK_SLOW_FALL_S),
            lift_rise: passing(ATTACK_LIFT_RISE_S),
            lift_keep: (-step / ATTACK_LIFT_FALL_S).exp(),
            floor: amplitude(ATTACK_SILENCE_DB * ATTACK_POWER),
        }
    }
}

fn under(x: f64, top: f64, knee: f64) -> f64 {
    let over = (x - top) / knee;
    if over > 40.0 {
        top
    } else if over < -40.0 {
        x
    } else {
        top - knee * (-over).exp().ln_1p()
    }
}

#[derive(Clone, Copy)]
struct Attack {
    fast: f64,
    slow: f64,
    lift: f64,
}

impl Attack {
    const RESTING: Attack = Attack { fast: 0.0, slow: 0.0, lift: 0.0 };

    fn lift(&mut self, level: f64, rates: &AttackRates) -> f64 {
        let power = level.powf(ATTACK_POWER);
        self.fast += (power - self.fast) * if power > self.fast { rates.fast_rise } else { rates.fast_fall };
        let blend = power + ATTACK_SLOW_BLEND * (self.fast - power);
        self.slow += (blend - self.slow) * if blend > self.slow { rates.slow_rise } else { rates.slow_fall };
        if self.fast < QUIET {
            self.fast = 0.0;
        }
        if self.slow < QUIET {
            self.slow = 0.0;
        }
        let fast_db = decibels(self.fast + rates.floor) / ATTACK_POWER;
        let gap = fast_db - decibels(self.slow + rates.floor) / ATTACK_POWER;
        let edge = under(gap, ATTACK_EDGE_TOP_DB, ATTACK_EDGE_KNEE_DB);
        let drive = ATTACK_EDGE_SLOPE * edge + ATTACK_GAP_SLOPE * gap + ATTACK_LEVEL_SLOPE * fast_db - ATTACK_BIAS_DB;
        let drive = under(drive, ATTACK_TOP_DB, ATTACK_TOP_KNEE_DB).max(0.0);
        self.lift = if drive > self.lift { self.lift + (drive - self.lift) * rates.lift_rise } else { drive + (self.lift - drive) * rates.lift_keep };
        if self.lift < QUIET {
            self.lift = 0.0;
        }
        self.lift
    }
}

struct SustainRates {
    quick_charge: f64,
    quick_slew: f64,
    quick_leak: f64,
    long_charge: f64,
    long_leak: f64,
    floor: f64,
}

impl SustainRates {
    fn at(step: f64) -> SustainRates {
        SustainRates {
            quick_charge: step * SUSTAIN_QUICK_KNEE_DB / SUSTAIN_QUICK_CHARGE_S,
            quick_slew: step * SUSTAIN_QUICK_SLEW_DB_PER_S,
            quick_leak: step / SUSTAIN_QUICK_LEAK_S,
            long_charge: -(-step / SUSTAIN_LONG_CHARGE_S).exp_m1(),
            long_leak: step / SUSTAIN_LONG_LEAK_S,
            floor: amplitude(SUSTAIN_SILENCE_DB),
        }
    }
}

#[derive(Clone, Copy)]
struct Sustain {
    quick: f64,
    long: f64,
}

impl Sustain {
    const RESTING: Sustain = Sustain { quick: SUSTAIN_QUICK_REST_DB, long: SUSTAIN_LONG_REST_DB };

    fn lift(&mut self, level: f64, rates: &SustainRates) -> f64 {
        let level = decibels(level + rates.floor) - SUSTAIN_ZERO_DB;
        let over = level - self.quick;
        let rise = if level > SUSTAIN_JUMP_EDGE_DB + SUSTAIN_JUMP_SLOPE * self.quick {
            SUSTAIN_JUMP_SHARE * over
        } else {
            let pull = rates.quick_charge * (over / SUSTAIN_QUICK_KNEE_DB).exp_m1();
            if pull > 0.0 { pull * rates.quick_slew / (pull + rates.quick_slew) } else { pull }
        };
        self.quick += rise - self.quick * rates.quick_leak;
        let held = ((self.long - level) / SUSTAIN_LONG_KNEE_DB).exp();
        self.long = level + SUSTAIN_LONG_KNEE_DB * (held + rates.long_charge * (1.0 - held)).ln() - self.long * rates.long_leak;
        (SUSTAIN_OFFSET_DB + SUSTAIN_SLOPE * (self.long - self.quick)).clamp(SUSTAIN_FLOOR_DB, SUSTAIN_CEILING_DB)
    }
}

pub struct Transient {
    sample_rate: f64,
    detector: Detector,
    attack: Attack,
    sustain: Sustain,
    knobs: Option<Knobs>,
    mix: Mix,
}

impl Unit for Transient {
    type Params = params::Transient;

    fn new(sample_rate: f64, _: usize) -> Transient {
        Transient { sample_rate, detector: Detector::SILENT, attack: Attack::RESTING, sustain: Sustain::RESTING, knobs: None, mix: Mix::new(sample_rate) }
    }

    fn restart(&mut self) {
        if self.attack.slow < STILL_POWER {
            (self.detector, self.attack, self.sustain) = (Detector::SILENT, Attack::RESTING, Sustain::RESTING);
        }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], p: &params::Transient, wet: f32, _: &Clock) {
        let n = l.len().min(r.len());
        dsp::sanitize(l, r);
        let sr = self.sample_rate;
        let step = p.speed / sr;
        let detector_keep = (-TAU * DETECTOR_LOW_CUT_HZ / sr).exp();
        let (attack_rates, sustain_rates) = (AttackRates::at(step), SustainRates::at(step));
        let knob_a = dsp::smoothing(KNOB_SMOOTH_S, sr) as f32;
        let target = Knobs::of(p);
        let mut knobs = self.knobs.unwrap_or(target);
        let wet = wet.clamp(0.0, 1.0);

        for i in 0..n {
            knobs.settle(&target, knob_a);
            let (dry_l, dry_r) = (l[i], r[i]);

            let level = self.detector.level(f64::from(dry_l), f64::from(dry_r), detector_keep);
            let attack = self.attack.lift(level, &attack_rates);
            let sustain = self.sustain.lift(level, &sustain_rates);

            let shaped_db = f64::from(knobs.attack) * attack + f64::from(knobs.sustain) * sustain;
            let gain = amplitude(shaped_db) as f32 * knobs.out_gain;
            let (mut wet_l, mut wet_r) = (dry_l * gain, dry_r * gain);
            if knobs.clip > 0.0 {
                wet_l += knobs.clip * (clip(wet_l) - wet_l);
                wet_r += knobs.clip * (clip(wet_r) - wet_r);
            }

            let mix = self.mix.tick(wet);
            l[i] = (1.0 - mix) * dry_l + mix * wet_l;
            r[i] = (1.0 - mix) * dry_r + mix * wet_r;
        }
        self.knobs = Some(knobs);
    }
}

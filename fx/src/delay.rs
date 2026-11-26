use sdk::dsp::noise::white;
use sdk::dsp::filter::{Coeffs, Filter, Path, State, Tuned};
use sdk::dsp::{DcBlock, Ring, SINC_HALF, db_to_gain, finite, follow, lp_coeff, one_pole_lp, sanitize, settle, smoothing, widen};
use crate::lfo_shape::{begun, seconds};
use crate::params::{self, DelayMode};
use crate::{Clock, Unit};
use std::f64::consts::PI;

const MAX_TIME_MS: f64 = params::Delay::MAX_TIME;
const MOD_CAP_MS: f64 = 30.0;
const SAFETY_MS: f64 = 100.0;
const FEEDBACK_CURVE: f32 = 0.65;
const FLUTTER_RATE_HZ: f64 = 6.0;
const FLUTTER_PHASE_OFFSET: f64 = 0.13;
const QUARTER_TURN: f32 = 0.25;
const MOD_DEPTH_FRACTION: f32 = 0.03;
const DIFFUSION_ALLPASS_MAX: f32 = 0.65;
const DIFFUSER_LENGTHS_AT_48K: [[usize; 2]; 3] = [[149, 263], [211, 293], [179, 277]];
const DIFFUSER_REFERENCE_RATE: f64 = 48_000.0;
const DIFFUSER_MIN_LEN: usize = 8;
const DIFFUSER_RING_SLACK: usize = 8;
const TAPE_LP_HZ: f64 = 7000.0;
const BBD_LP_HZ: f64 = 4000.0;
const BBD_NOISE_AMP: f32 = 3.16e-4;
const DC_BLOCKER_HZ: f64 = 35.0;
const DETECT_ATTACK_SECONDS: f64 = 0.001;
const DETECT_RELEASE_SECONDS: f64 = 0.050;
const SMOOTH_SECONDS: f64 = 0.030;
const CENTRE_FLOOR: f32 = 1e-4;
const FREEZE_SNAP: f32 = 0.9999;
const ANTI_DENORM: f32 = 1e-20;
const WRITE_CLAMP: f32 = 4.0;
const RNG_SEED: u32 = 0x1234_5678;
const SPLICE_REACH: f64 = 2.5;
const SPLICE_GUARD: f64 = (SINC_HALF + 2) as f64;
const SPLICE_SLACK: usize = 2 * SINC_HALF + 8;
const LINE_GUARD: usize = 2 * SINC_HALF + 4;
const SINC_FOLD: f64 = 0.9;
const LIVE_FLOOR: f32 = 1e-3;
const TAPS: usize = 3;
const FILTERS: usize = params::Delay::FILTERS;
const FEEDBACK_FILTER: usize = 2;
const GUARD_CEILING: f32 = 1.0;
const GUARD_ATTACK_SECONDS: f64 = 0.001;
const GUARD_RELEASE_SECONDS: f64 = 0.200;
const GUARD_SMOOTH_SECONDS: f64 = 0.010;
const FOLLOW_FLOOR: f32 = 1e-6;
const FOLLOW_GAIN_MAX: f32 = 2.0;
const LOOP_SOFT_LEVEL: f32 = 2.3;
const WET_BEND: f32 = 4.0;
const BEND_FLOOR: f32 = 1e-6;
const QUIET_FLOOR: f32 = 1e-6;

fn softened(x: f32) -> f32 {
    LOOP_SOFT_LEVEL * (x / LOOP_SOFT_LEVEL).tanh()
}

fn bent(x: f32, drive: f32) -> f32 {
    if drive > BEND_FLOOR {
        let knee = WET_BEND * drive;
        (knee * x).tanh() / knee
    } else {
        x
    }
}

struct Derived {
    from: [u64; 11],
    feedback_gain: f32,
    feedback_gain_r: f32,
    wet_gain: f32,
    ratio_l: f32,
    ratio_r: f32,
    duck_attack: f32,
    duck_release: f32,
    follow_attack: f32,
    follow_release: f32,
    threshold: f32,
}

impl Derived {
    fn inputs(p: &params::Delay) -> [u64; 11] {
        [p.feedback_ceiling, p.feedback, p.feedback_r.unwrap_or(p.feedback), p.wet_trim_db, p.pitch, p.detune, p.attack_ms, p.release_ms, p.follow_attack_ms, p.follow_release_ms, p.threshold_db].map(f64::to_bits)
    }

    fn of(p: &params::Delay, sample_rate: f64) -> Derived {
        let detune = p.detune / 100.0;
        Derived {
            from: Derived::inputs(p),
            feedback_gain: p.feedback_ceiling as f32 * (p.feedback as f32).powf(FEEDBACK_CURVE),
            feedback_gain_r: p.feedback_ceiling as f32 * (p.feedback_r.unwrap_or(p.feedback) as f32).powf(FEEDBACK_CURVE),
            wet_gain: db_to_gain(p.wet_trim_db) as f32,
            ratio_l: ((p.pitch - detune) / 12.0).exp2() as f32,
            ratio_r: ((p.pitch + detune) / 12.0).exp2() as f32,
            duck_attack: smoothing(p.attack_ms * 0.001, sample_rate) as f32,
            duck_release: smoothing(p.release_ms * 0.001, sample_rate) as f32,
            follow_attack: smoothing(p.follow_attack_ms * 0.001, sample_rate) as f32,
            follow_release: smoothing(p.follow_release_ms * 0.001, sample_rate) as f32,
            threshold: db_to_gain(p.threshold_db) as f32,
        }
    }
}

struct Rates {
    approach: f32,
    detect_attack: f32,
    detect_release: f32,
    guard_attack: f32,
    guard_release: f32,
    guard_smooth: f32,
    tape_alpha: f32,
    bbd_alpha: f32,
    dc_pole: f32,
}

impl Rates {
    fn at(sample_rate: f64) -> Rates {
        Rates {
            approach: smoothing(SMOOTH_SECONDS, sample_rate) as f32,
            detect_attack: smoothing(DETECT_ATTACK_SECONDS, sample_rate) as f32,
            detect_release: smoothing(DETECT_RELEASE_SECONDS, sample_rate) as f32,
            guard_attack: smoothing(GUARD_ATTACK_SECONDS, sample_rate) as f32,
            guard_release: smoothing(GUARD_RELEASE_SECONDS, sample_rate) as f32,
            guard_smooth: smoothing(GUARD_SMOOTH_SECONDS, sample_rate) as f32,
            tape_alpha: lp_coeff(TAPE_LP_HZ, sample_rate),
            bbd_alpha: lp_coeff(BBD_LP_HZ, sample_rate),
            dc_pole: lp_coeff(DC_BLOCKER_HZ, sample_rate),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
struct Knobs {
    tap_l: f32,
    tap_r: f32,
    tap_c: f32,
    centre: f32,
    pan_c: f32,
    feedback_gain: f32,
    feedback_gain_r: f32,
    ping_pong: f32,
    ping_pong_r: f32,
    even: f32,
    lean: f32,
    drive: f32,
    filters: [Coeffs; FILTERS],
    modulation: f32,
    wow_rate: f32,
    flutter: f32,
    spread: f32,
    diffusion: f32,
    width: f32,
    pan: f32,
    pan_l: f32,
    pan_r: f32,
    ducking: f32,
    follow_threshold_db: f32,
    follow_range_db: f32,
    follow_feedback: f32,
    follow_feedback_r: f32,
    follow_cross: f32,
    follow_cross_r: f32,
    follow_wet: f32,
    follow_dry: f32,
    wet_gain: f32,
    wet_drive: f32,
    input_drive: f32,
    mix: f32,
    keep: f32,
    forward: f32,
    ratio_l: f32,
    ratio_r: f32,
    splice: f32,
    hold: f32,
    freeze: f32,
}

impl Knobs {
    fn target(p: &params::Delay, derived: &Derived, filters: [Coeffs; FILTERS], wet: f32, clock: &Clock, sample_rate: f64) -> Knobs {
        let time_l_ms = (if p.sync_l { clock.ms(p.division_l) * p.scale_l } else { p.time_l_ms }).clamp(1.0, MAX_TIME_MS);
        let time_r_ms = (if p.link { time_l_ms } else if p.sync_r { clock.ms(p.division_r) * p.scale_r } else { p.time_r_ms }).clamp(1.0, MAX_TIME_MS);
        let time_c_ms = (if p.sync_c { clock.ms(p.division_c) } else { p.time_c_ms }).clamp(1.0, MAX_TIME_MS);
        Knobs {
            tap_l: (time_l_ms * 0.001 * sample_rate) as f32,
            tap_r: (time_r_ms * 0.001 * sample_rate) as f32,
            tap_c: (time_c_ms * 0.001 * sample_rate) as f32,
            centre: p.level_c as f32,
            pan_c: p.pan_c as f32,
            feedback_gain: derived.feedback_gain,
            feedback_gain_r: derived.feedback_gain_r,
            ping_pong: p.ping_pong as f32,
            ping_pong_r: p.ping_pong_r.unwrap_or(p.ping_pong) as f32,
            even: f32::from(u8::from(p.even)),
            lean: p.lean as f32,
            drive: p.drive as f32,
            filters,
            modulation: p.mod_depth as f32,
            wow_rate: p.mod_rate_hz as f32,
            flutter: p.mod_flutter as f32,
            spread: p.mod_spread as f32,
            diffusion: p.diffusion as f32,
            width: p.width as f32,
            pan: p.pan as f32,
            pan_l: p.pan_l as f32,
            pan_r: p.pan_r as f32,
            ducking: p.ducking as f32,
            follow_threshold_db: p.follow_threshold_db as f32,
            follow_range_db: p.follow_range_db as f32,
            follow_feedback: p.follow_feedback as f32,
            follow_feedback_r: p.follow_feedback_r as f32,
            follow_cross: p.follow_cross as f32,
            follow_cross_r: p.follow_cross_r as f32,
            follow_wet: p.follow_wet as f32,
            follow_dry: p.follow_dry as f32,
            wet_gain: derived.wet_gain,
            wet_drive: p.wet_drive as f32,
            input_drive: p.input_drive as f32,
            mix: wet,
            keep: p.dry as f32,
            forward: p.forward as f32,
            ratio_l: derived.ratio_l,
            ratio_r: derived.ratio_r,
            splice: p.splice as f32,
            hold: f32::from(u8::from(p.splice > 0.0 && p.hold)),
            freeze: f32::from(u8::from(p.freeze)),
        }
    }

    fn approach(&mut self, target: &Knobs, a: f32) {
        self.tap_l = settle(self.tap_l, target.tap_l, a);
        self.tap_r = settle(self.tap_r, target.tap_r, a);
        self.tap_c = settle(self.tap_c, target.tap_c, a);
        self.centre = settle(self.centre, target.centre, a);
        self.pan_c = settle(self.pan_c, target.pan_c, a);
        self.feedback_gain = settle(self.feedback_gain, target.feedback_gain, a);
        self.feedback_gain_r = settle(self.feedback_gain_r, target.feedback_gain_r, a);
        self.ping_pong = settle(self.ping_pong, target.ping_pong, a);
        self.ping_pong_r = settle(self.ping_pong_r, target.ping_pong_r, a);
        self.even = settle(self.even, target.even, a);
        self.lean = settle(self.lean, target.lean, a);
        self.drive = settle(self.drive, target.drive, a);
        for (mine, theirs) in self.filters.iter_mut().zip(&target.filters) {
            mine.approach(theirs, a);
        }
        self.modulation = settle(self.modulation, target.modulation, a);
        self.wow_rate = settle(self.wow_rate, target.wow_rate, a);
        self.flutter = settle(self.flutter, target.flutter, a);
        self.spread = settle(self.spread, target.spread, a);
        self.diffusion = settle(self.diffusion, target.diffusion, a);
        self.width = settle(self.width, target.width, a);
        self.pan = settle(self.pan, target.pan, a);
        self.pan_l = settle(self.pan_l, target.pan_l, a);
        self.pan_r = settle(self.pan_r, target.pan_r, a);
        self.ducking = settle(self.ducking, target.ducking, a);
        self.follow_threshold_db = settle(self.follow_threshold_db, target.follow_threshold_db, a);
        self.follow_range_db = settle(self.follow_range_db, target.follow_range_db, a);
        self.follow_feedback = settle(self.follow_feedback, target.follow_feedback, a);
        self.follow_feedback_r = settle(self.follow_feedback_r, target.follow_feedback_r, a);
        self.follow_cross = settle(self.follow_cross, target.follow_cross, a);
        self.follow_cross_r = settle(self.follow_cross_r, target.follow_cross_r, a);
        self.follow_wet = settle(self.follow_wet, target.follow_wet, a);
        self.follow_dry = settle(self.follow_dry, target.follow_dry, a);
        self.wet_gain = settle(self.wet_gain, target.wet_gain, a);
        self.wet_drive = settle(self.wet_drive, target.wet_drive, a);
        self.input_drive = settle(self.input_drive, target.input_drive, a);
        self.mix = settle(self.mix, target.mix, a);
        self.keep = settle(self.keep, target.keep, a);
        self.forward = settle(self.forward, target.forward, a);
        self.ratio_l = settle(self.ratio_l, target.ratio_l, a);
        self.ratio_r = settle(self.ratio_r, target.ratio_r, a);
        self.splice = settle(self.splice, target.splice, a);
        self.hold = settle(self.hold, target.hold, a);
        self.freeze = settle(self.freeze, target.freeze, a);
    }

    fn following(&self) -> bool {
        [self.follow_feedback, self.follow_feedback_r, self.follow_cross, self.follow_cross_r, self.follow_wet, self.follow_dry].iter().any(|amount| *amount != 0.0)
    }

    fn holding(&self) -> bool {
        self.freeze > 0.0 || self.hold > 0.0
    }
}

#[derive(Clone, Copy)]
struct Saturation {
    drive: f32,
    gain: f32,
}

impl Saturation {
    fn of(drive: f32) -> Saturation {
        let gain = 1.0 + 9.0 * drive * drive;
        Saturation { drive, gain }
    }

    fn at(self, drive: f32) -> Saturation {
        if drive == self.drive { self } else { Saturation::of(drive) }
    }
}

#[derive(Default)]
struct Coloring {
    tape_lp: f32,
    bbd_lp: f32,
    dc: DcBlock,
}

impl Coloring {
    fn tape(&mut self, raw: f32, saturation: Saturation, alpha: f32, pole: f32) -> f32 {
        let saturated = (saturation.gain * raw).tanh() / saturation.gain;
        self.tape_lp = one_pole_lp(saturated, alpha, self.tape_lp);
        self.dc.tick(self.tape_lp, pole)
    }

    fn bbd(&mut self, raw: f32, drive: f32, alpha: f32, noise: f32, pole: f32) -> f32 {
        let gain = 1.0 + drive;
        let compressed = (raw.abs().sqrt() * gain).min(1.0).copysign(raw);
        self.bbd_lp = one_pole_lp(compressed, alpha, self.bbd_lp);
        let noisy = self.bbd_lp + noise;
        self.dc.tick((noisy * noisy).copysign(noisy) / (gain * gain), pole)
    }
}

struct Tap {
    delay: f64,
    consumed: f64,
    slice: f64,
    fade: f64,
    age: f64,
    fall: f64,
    left: Option<f64>,
}

impl Tap {
    fn born(slice: f64, fade: f64, ratio: f64, reverse: bool, held: bool, risen: bool) -> Tap {
        let delay = if reverse { 0.0 } else if held { slice + ratio * fade } else { slice };
        Tap { delay, consumed: 0.0, slice, fade, age: if risen { fade } else { 0.0 }, fall: fade, left: None }
    }

    fn sprung(delay: f64, fade: f64, risen: bool) -> Tap {
        Tap { delay, consumed: 0.0, slice: 0.0, fade, age: if risen { fade } else { 0.0 }, fall: fade, left: None }
    }

    fn room(&self, drift: f64, reach: f64) -> f64 {
        if drift < 0.0 {
            self.delay / -drift
        } else if drift > 0.0 {
            (reach - self.delay) / drift
        } else {
            f64::INFINITY
        }
    }

    fn cut(&mut self, drift: f64, reach: f64) {
        self.yields(self.fade, drift, reach);
    }

    fn yields(&mut self, fade: f64, drift: f64, reach: f64) {
        if self.left.is_none() {
            self.fall = fade.min(self.room(drift, reach)).max(1.0);
            self.left = Some(self.fall);
        }
    }

    fn ended(&self, span: f64, fade: f64, drift: f64, reach: f64) -> bool {
        self.room(drift, reach) <= fade || (drift > 0.0 && self.delay >= span)
    }

    fn gain(&self) -> f32 {
        let rise = (self.age / self.fade).min(1.0);
        let fall = self.left.map_or(1.0, |left| left / self.fall);
        (rise * fall) as f32
    }

    fn counted(&mut self) -> bool {
        match &mut self.left {
            Some(left) => {
                *left -= 1.0;
                *left > 0.0
            }
            None => true,
        }
    }

    fn glide(&mut self, drift: f64, reach: f64) -> bool {
        self.age += 1.0;
        self.delay = (self.delay + drift).clamp(0.0, reach);
        self.counted()
    }

    fn advance(&mut self, ratio: f64, drift: f64, reach: f64, bridge: bool) -> bool {
        self.age += 1.0;
        self.consumed += ratio;
        self.delay = (self.delay + drift).clamp(0.0, reach);
        if self.left.is_none() && (self.room(drift, reach) <= self.fade || (self.consumed >= self.slice && !bridge)) {
            self.cut(drift, reach);
        }
        self.counted()
    }
}

struct Line {
    ring: Ring,
    filled: usize,
}

impl Line {
    fn new(samples: usize) -> Line {
        Line { ring: Ring::new(samples), filled: usize::MAX }
    }

    fn push(&mut self, x: f32) {
        self.ring.push(x);
        self.filled = self.filled.saturating_add(1);
    }

    fn clear(&mut self) {
        for _ in 0..LINE_GUARD {
            self.ring.push(0.0);
        }
        self.filled = LINE_GUARD;
    }

    fn holds(&self, back: f64) -> bool {
        back <= self.filled as f64
    }

    fn tap(&self, delay: usize) -> f32 {
        if delay <= self.filled { self.ring.tap(delay) } else { 0.0 }
    }

    fn hermite(&self, delay: f64) -> f32 {
        if self.holds(delay.floor() + 2.0) { self.ring.hermite(delay) } else { 0.0 }
    }

    fn bandlimited(&self, delay: f64, cutoff: f64) -> f32 {
        if self.holds(delay.floor() + SINC_HALF as f64) { self.ring.bandlimited(delay, cutoff) } else { 0.0 }
    }
}

fn heard(line: &Line, delay: f64, cutoff: f64, plain: bool) -> f32 {
    if plain { line.hermite(SPLICE_GUARD + delay) } else { line.bandlimited(SPLICE_GUARD + delay, cutoff) }
}

struct Splice {
    ring: Line,
    reach: f64,
    taps: [Option<Tap>; TAPS],
    deepest: f64,
}

impl Splice {
    fn new(sample_rate: f64) -> Splice {
        let reach = SPLICE_REACH * params::Delay::MAX_SPLICE * 0.001 * sample_rate;
        Splice { ring: Line::new(reach.ceil() as usize + SPLICE_SLACK), reach, taps: [None, None, None], deepest: 0.0 }
    }

    fn write(&mut self, x: f32) {
        self.ring.push(x);
    }

    fn free(&self) -> usize {
        self.taps.iter().position(Option::is_none).unwrap_or_else(|| {
            let ages = self.taps.iter().map(|tap| tap.as_ref().map_or(f64::INFINITY, |tap| tap.age));
            ages.enumerate().fold((0, 0.0), |(at, oldest), (slot, age)| if age > oldest { (slot, age) } else { (at, oldest) }).0
        })
    }

    fn birth(&mut self, slice: f64, fade: f64, ratio: f64, drift: f64, reverse: bool, held: bool, risen: bool) {
        for tap in self.taps.iter_mut().flatten() {
            tap.cut(drift, self.reach);
        }
        let slot = self.free();
        self.taps[slot] = Some(Tap::born(slice, fade, ratio, reverse, held, risen));
    }

    fn read(&mut self, ratio: f64, drift: f64, bridge: bool, plain: bool) -> f32 {
        let cutoff = (SINC_FOLD / ratio).min(1.0);
        let mut sum = 0.0;
        for slot in &mut self.taps {
            if let Some(tap) = slot {
                self.deepest = self.deepest.max(tap.delay.min(self.reach));
                sum += heard(&self.ring, tap.delay, cutoff, plain) * tap.gain();
                if !tap.advance(ratio, drift, self.reach, bridge) {
                    *slot = None;
                }
            }
        }
        sum
    }

    fn sweep(&mut self, span: f64, fade: f64, ratio: f64, drift: f64, primed: bool, plain: bool) -> f32 {
        let fade = if drift == 0.0 { fade } else { fade.min(span / drift.abs()) }.max(1.0);
        if primed {
            self.hush();
        }
        let lead = self.taps.iter().flatten().find(|tap| tap.left.is_none());
        if lead.is_none_or(|tap| tap.ended(span, fade, drift, self.reach)) {
            for tap in self.taps.iter_mut().flatten() {
                tap.yields(fade, drift, self.reach);
            }
            let start = if drift > 0.0 { 0.0 } else if drift < 0.0 { span - drift * fade } else { 0.5 * span };
            let along = if primed && drift != 0.0 { 0.5 * span * drift.signum() } else { 0.0 };
            let slot = self.free();
            self.taps[slot] = Some(Tap::sprung(start + along, fade, primed));
        }
        let cutoff = (SINC_FOLD / ratio).min(1.0);
        let mut sum = 0.0;
        for slot in &mut self.taps {
            if let Some(tap) = slot {
                self.deepest = self.deepest.max(tap.delay.min(self.reach));
                sum += heard(&self.ring, tap.delay, cutoff, plain) * tap.gain();
                if !tap.glide(drift, self.reach) {
                    *slot = None;
                }
            }
        }
        sum
    }

    fn hush(&mut self) {
        self.taps = [None, None, None];
        self.deepest = 0.0;
    }

    fn clear(&mut self) {
        self.ring.clear();
        self.hush();
    }
}

#[derive(Default)]
struct Cadence {
    since: f64,
    slot: Option<i64>,
}

impl Cadence {
    fn tick(&mut self, period: f64, bar: Option<(f64, f64)>, downbeat: bool) -> (bool, f64) {
        match bar {
            Some((phase, frames)) => {
                let slot = (phase / period).floor() as i64;
                let due = downbeat || self.slot.is_some_and(|last| last != slot);
                self.slot = Some(slot);
                self.since = phase - slot as f64 * period;
                (due, (frames - phase).min((slot + 1) as f64 * period - phase))
            }
            None => {
                self.slot = None;
                self.since += 1.0;
                let due = self.since >= period;
                if due {
                    self.since -= period;
                }
                (due, period - self.since)
            }
        }
    }
}

#[derive(Default)]
struct Tone([State; 2]);

impl Tone {
    fn run(&mut self, x: f32, knobs: &Knobs, paths: &[Path; FILTERS]) -> f32 {
        let cut = self.0[0].tick(x, paths[0], &knobs.filters[0]);
        self.0[1].tick(cut, paths[1], &knobs.filters[1])
    }
}

struct Steady {
    coloring: Coloring,
    tone: Tone,
    diffusers: [Ring; 2],
}

impl Steady {
    fn rest(&mut self) {
        self.coloring = Coloring::default();
        self.tone = Tone::default();
        for diffuser in &mut self.diffusers {
            diffuser.clear();
        }
    }
}

fn smear(diffusers: &mut [Ring; 2], lengths: [usize; 2], x: f32, amount: f32) -> f32 {
    let g = DIFFUSION_ALLPASS_MAX * amount;
    let [first, second] = lengths.map(|len| ((len as f32 * amount).round() as usize).max(1));
    let y = diffusers[0].allpass(first, g, x);
    let y = diffusers[1].allpass(second, g, y);
    if amount > 0.0 { y } else { x }
}

fn spliced(toned: f32, grain: Option<f32>, blend: f32) -> f32 {
    grain.map_or(toned, |grain| toned + (grain - toned) * blend)
}

struct Even {
    aside: Line,
    first: Line,
    aside_tone: Tone,
    first_tone: Tone,
    steady: Steady,
}

impl Even {
    fn new(sample_rate: f64, lengths: [usize; 2]) -> Even {
        Even {
            aside: Line::new(((MAX_TIME_MS + MOD_CAP_MS + SAFETY_MS) * 0.001 * sample_rate).ceil() as usize),
            first: Line::new(((MAX_TIME_MS + MOD_CAP_MS + SAFETY_MS) * 0.001 * sample_rate).ceil() as usize),
            aside_tone: Tone::default(),
            first_tone: Tone::default(),
            steady: Steady { coloring: Coloring::default(), tone: Tone::default(), diffusers: lengths.map(|len| Ring::new(len + DIFFUSER_RING_SLACK)) },
        }
    }

    fn aside_read(&self, delay: f32) -> f32 {
        finite(self.aside.hermite(f64::from(delay.max(1.0))))
    }

    fn first_read(&self, delay: f32) -> f32 {
        finite(self.first.hermite(f64::from(delay.max(1.0))))
    }

    fn crossing(&mut self, delay: f32, knobs: &Knobs, paths: &[Path; FILTERS]) -> (f32, f32) {
        let through = self.aside_tone.run(self.aside_read(delay), knobs, paths);
        let passed = self.first_tone.run(self.first_read(delay), knobs, paths);
        (through, passed)
    }

    fn steady_echo(&mut self, raw: f32, lengths: [usize; 2], colour: impl FnOnce(&mut Coloring, f32) -> f32, knobs: &Knobs, paths: &[Path; FILTERS], after: bool, grain: Option<f32>, blend: f32) -> f32 {
        let coloured = colour(&mut self.steady.coloring, raw);
        let echo = if after {
            self.steady.tone.run(spliced(coloured, grain, blend), knobs, paths)
        } else {
            spliced(self.steady.tone.run(coloured, knobs, paths), grain, blend)
        };
        smear(&mut self.steady.diffusers, lengths, echo, knobs.diffusion)
    }

    fn clear_crossing(&mut self) {
        self.aside.clear();
        self.first.clear();
        self.aside_tone = Tone::default();
        self.first_tone = Tone::default();
    }

    fn reset_filters(&mut self) {
        self.aside_tone = Tone::default();
        self.first_tone = Tone::default();
        self.steady.rest();
    }

    fn clear(&mut self) {
        self.aside.clear();
        self.first.clear();
        self.reset_filters();
    }
}

struct Channel {
    ring: Line,
    diffusers: [Ring; 2],
    lengths: [usize; 2],
    coloring: Coloring,
    tone: Tone,
    back: State,
    splice: Splice,
    cadence: Cadence,
}

impl Channel {
    fn new(sample_rate: f64, lengths_at_48k: [usize; 2]) -> Channel {
        let lengths = lengths_at_48k.map(|len| ((len as f64 * sample_rate / DIFFUSER_REFERENCE_RATE).round() as usize).max(DIFFUSER_MIN_LEN));
        Channel {
            ring: Line::new(((MAX_TIME_MS + MOD_CAP_MS + SAFETY_MS) * 0.001 * sample_rate).ceil() as usize),
            diffusers: lengths.map(|len| Ring::new(len + DIFFUSER_RING_SLACK)),
            lengths,
            coloring: Coloring::default(),
            tone: Tone::default(),
            back: State::default(),
            splice: Splice::new(sample_rate),
            cadence: Cadence::default(),
        }
    }

    fn read(&self, delay: f32) -> f32 {
        finite(self.ring.hermite(f64::from(delay.max(1.0))))
    }

    fn frozen(&self, delay: f32) -> f32 {
        self.ring.tap(delay.round() as usize)
    }

    fn tone(&mut self, x: f32, knobs: &Knobs, paths: &[Path; FILTERS]) -> f32 {
        self.tone.run(x, knobs, paths)
    }

    fn shared_tone(&mut self, x: f32, share: f32, knobs: &Knobs, paths: &[Path; FILTERS]) -> f32 {
        let cut = self.tone.run(x, knobs, paths);
        if share > 0.0 { x + (1.0 - share) * (cut - x) } else { cut }
    }

    fn loop_tone(&mut self, x: f32, knobs: &Knobs, paths: &[Path; FILTERS]) -> f32 {
        self.back.tick(x, paths[FEEDBACK_FILTER], &knobs.filters[FEEDBACK_FILTER])
    }

    fn splice(&mut self, toned: f32, period: f64, fade: f64, ratio: f32, sway: &Sway, bar: Option<(f64, f64)>, downbeat: bool, primed: bool) -> Option<f32> {
        let (due, remaining) = self.cadence.tick(period, bar, downbeat);
        if !sway.live {
            self.splice.write(toned);
            return None;
        }
        let ratio = f64::from(ratio);
        let drift = if sway.reverse { sway.head + ratio } else { sway.head - ratio };
        if sway.sweep {
            if !sway.held {
                self.splice.write(toned);
            }
            return Some(self.splice.sweep(period, fade, ratio, drift, primed, sway.plain));
        }
        let fade = fade.min(period / 2.0).max(1.0);
        if due || primed {
            self.splice.birth(period, fade, ratio, drift, sway.reverse, sway.held, primed);
        }
        if !sway.held {
            self.splice.write(toned);
        }
        Some(self.splice.read(ratio, drift, remaining <= fade, sway.plain))
    }

    fn diffuse(&mut self, x: f32, amount: f32) -> f32 {
        smear(&mut self.diffusers, self.lengths, x, amount)
    }

    fn reset_filters(&mut self) {
        self.coloring = Coloring::default();
        self.tone = Tone::default();
        self.back = State::default();
    }

    fn clear(&mut self) {
        self.ring.clear();
        for diffuser in &mut self.diffusers {
            diffuser.clear();
        }
        self.reset_filters();
        self.splice.clear();
    }
}

struct Sway {
    live: bool,
    held: bool,
    head: f64,
    reverse: bool,
    sweep: bool,
    plain: bool,
    blend: f32,
}

fn leaned(l: f32, r: f32, lean: f32) -> (f32, f32) {
    let s = lean.abs();
    if lean < 0.0 { ((1.0 + s) * l + s * r, (1.0 - s) * r) } else { ((1.0 - s) * l, (1.0 + s) * r + s * l) }
}

fn evened(l: f32, r: f32, share: f32) -> (f32, f32) {
    let mono = 0.5 * (l + r);
    (l + share * (mono - l), r - share * r)
}

fn quadrature(turns: f64) -> (f32, f32) {
    let (sin, cos) = (2.0 * PI * turns).sin_cos();
    (sin as f32, cos as f32)
}

fn spread_apart(turns: f64, spread: f32) -> (f32, f32) {
    if spread == QUARTER_TURN { quadrature(turns) } else { ((2.0 * PI * turns).sin() as f32, (2.0 * PI * (turns + f64::from(spread))).sin() as f32) }
}

pub struct Delay {
    sample_rate: f64,
    left: Channel,
    right: Channel,
    centre: Channel,
    even: Even,
    centred: bool,
    knobs: Option<Knobs>,
    derived: Derived,
    filters: [Tuned; FILTERS],
    rates: Rates,
    saturation: Saturation,
    mode: DelayMode,
    restart_pending: bool,
    wow_phase: f64,
    flutter_phase: f64,
    duck_env: f32,
    duck_active: f32,
    follow_env: f32,
    guard_env: f32,
    guard: f32,
    tickle_sign: f32,
    rng: u32,
    step: Option<i64>,
    primed: bool,
    stereo: bool,
    steady: bool,
    quiet: usize,
    empty: usize,
}

impl Delay {
    fn clear(&mut self) {
        self.left.clear();
        self.right.clear();
        self.centre.clear();
        self.even.clear();
        self.duck_env = 0.0;
        self.duck_active = 0.0;
        self.follow_env = 0.0;
        self.guard_env = 0.0;
        self.guard = 1.0;
        self.primed = true;
    }

    fn renew(&mut self) {
        self.clear();
        for channel in [&mut self.left, &mut self.right, &mut self.centre] {
            channel.cadence = Cadence::default();
        }
        self.tickle_sign = 1.0;
        self.rng = RNG_SEED;
        self.empty = usize::MAX;
    }

    fn silent(&self, knobs: &Knobs) -> bool {
        let taps = f64::from(knobs.tap_l.max(knobs.tap_r).max(if self.centred { knobs.tap_c } else { 0.0 }));
        let splices = self.left.splice.deepest.max(self.right.splice.deepest).max(self.centre.splice.deepest);
        let margin = (MOD_CAP_MS + SAFETY_MS) * 0.001 * self.sample_rate;
        let (quiet, empty) = (self.quiet as f64, self.empty as f64);
        quiet >= margin && (!knobs.holding() || empty >= taps.max(splices) + margin)
    }
}

impl Unit for Delay {
    type Params = params::Delay;

    fn new(sample_rate: f64, _: usize) -> Delay {
        let left = Channel::new(sample_rate, DIFFUSER_LENGTHS_AT_48K[0]);
        Delay {
            sample_rate,
            even: Even::new(sample_rate, left.lengths),
            left,
            right: Channel::new(sample_rate, DIFFUSER_LENGTHS_AT_48K[1]),
            centre: Channel::new(sample_rate, DIFFUSER_LENGTHS_AT_48K[2]),
            centred: false,
            knobs: None,
            derived: Derived::of(&params::Delay::DEFAULT, sample_rate),
            filters: params::Delay::DEFAULT.filters().map(|filter| Tuned::of(&filter, sample_rate)),
            rates: Rates::at(sample_rate),
            saturation: Saturation::of(params::Delay::DEFAULT.drive as f32),
            mode: params::Delay::DEFAULT.mode,
            restart_pending: true,
            wow_phase: 0.0,
            flutter_phase: 0.0,
            duck_env: 0.0,
            duck_active: 0.0,
            follow_env: 0.0,
            guard_env: 0.0,
            guard: 1.0,
            tickle_sign: 1.0,
            rng: RNG_SEED,
            step: None,
            primed: true,
            stereo: false,
            steady: false,
            quiet: usize::MAX,
            empty: usize::MAX,
        }
    }

    fn restart(&mut self) {
        self.restart_pending = true;
        if self.knobs.is_some_and(|knobs| self.silent(&knobs)) {
            self.renew();
            self.knobs = None;
        }
    }

    fn holding(&self) -> bool {
        self.knobs.is_some_and(|knobs| knobs.holding())
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], params: &params::Delay, wet: f32, clock: &Clock) {
        sanitize(l, r);
        let sr = self.sample_rate;
        if Derived::inputs(params) != self.derived.from {
            self.derived = Derived::of(params, sr);
        }
        for (tuned, filter) in self.filters.iter_mut().zip(params.filters()) {
            tuned.tune(&filter, sr);
        }
        let paths = self.filters.map(|tuned| tuned.path);
        let ringing = params.filters().iter().any(Filter::rings);
        let target = Knobs::target(params, &self.derived, self.filters.map(|tuned| tuned.coeffs), wet, clock, sr);
        let Derived { duck_attack, duck_release, follow_attack, follow_release, threshold, .. } = self.derived;
        let Rates { approach, detect_attack, detect_release, guard_attack, guard_release, guard_smooth, tape_alpha, bbd_alpha, dc_pole } = self.rates;
        let lifted = params.feedback_ceiling > params::Delay::FEEDBACK_CEILING;
        let mod_cap = (MOD_CAP_MS * 0.001 * sr) as f32;
        let flutter_step = FLUTTER_RATE_HZ / sr;
        let splice_ms = if params.splice_sync { clock.ms(params.splice_division) } else { params.splice_ms };
        let parts = (splice_ms / params::Delay::MAX_SPLICE).ceil().max(1.0);
        let shortest = params::Delay::MIN_SPLICE * 0.001 * sr;
        let grid = params.splice_sync.then_some(params.splice_division.beats() / parts).zip(clock.beat);
        let period_l = grid.map_or((splice_ms / parts) * 0.001 * sr, |(beats, beat)| beats * beat.frames_per_beat).max(shortest);
        let period_r = (period_l * (1.0 - params.splice_offset)).max(shortest);
        let fade = params.smoothing_ms * 0.001 * sr;

        if params.mode != self.mode {
            self.mode = params.mode;
            self.left.coloring = Coloring::default();
            self.right.coloring = Coloring::default();
            self.centre.coloring = Coloring::default();
            self.even.steady.coloring = Coloring::default();
        }

        let mut knobs = self.knobs.unwrap_or(target);
        let start = knobs;
        let n = l.len().min(r.len());
        if n > 0 && self.restart_pending {
            self.restart_pending = false;
            let at = seconds(clock);
            self.wow_phase = begun(params.start_phase, at * f64::from(params.mod_rate_hz as f32)).1;
            self.flutter_phase = begun(params.start_phase + FLUTTER_PHASE_OFFSET, at * FLUTTER_RATE_HZ).1.fract();
            self.step = None;
        }
        let (mut wow, mut flutter) = (self.wow_phase, self.flutter_phase);
        let mut duck_env = self.duck_env;
        let mut duck_active = self.duck_active;
        let mut follow_env = self.follow_env;
        let mut guard_env = self.guard_env;
        let mut guard = self.guard;
        let mut tickle_sign = self.tickle_sign;
        let mut rng = self.rng;
        let mut quiet = self.quiet;
        let mut empty = self.empty;
        let mut blown = false;
        let mut still = false;

        for i in 0..n {
            if !still {
                knobs.approach(&target, approach);
                if params.freeze && knobs.freeze > FREEZE_SNAP {
                    knobs.freeze = 1.0;
                }
                still = i == 0 && knobs == start;
            }
            let (dry_l, dry_r) = (l[i], r[i]);
            let key = dry_l.abs().max(dry_r.abs());
            let following = knobs.following();
            let ef = if following {
                follow_env = follow(follow_env, key, follow_attack, follow_release);
                ((20.0 * follow_env.max(FOLLOW_FLOOR).log10() - knobs.follow_threshold_db) / knobs.follow_range_db).clamp(0.0, 1.0)
            } else {
                follow_env = 0.0;
                0.0
            };

            let step = grid.map(|(beats, beat)| (beat.at(i) / beats).floor() as i64);
            let downbeat = matches!((self.step, step), (Some(last), Some(now)) if last != now);
            self.step = step;
            let bar = grid.zip(step).map(|((beats, beat), step)| ((beat.at(i) - step as f64 * beats) * beat.frames_per_beat, beats * beat.frames_per_beat));
            let live = params.splice > 0.0 || knobs.splice > LIVE_FLOOR;
            let held = live && params.hold;
            let sway = Sway { live, held, head: if held { 0.0 } else { 1.0 }, reverse: params.splice_reverse, sweep: params.sweep, plain: params.aliasing, blend: knobs.splice };
            if !live {
                self.left.splice.hush();
                self.right.splice.hush();
                self.centre.splice.hush();
                self.primed = true;
            }

            let share = knobs.even * knobs.ping_pong;
            let steady = params.even_sway && share > 0.0;
            if steady && !self.steady {
                self.even.steady.rest();
            }
            self.steady = steady;
            let depth = knobs.modulation * knobs.modulation;
            let (delay_l, delay_r) = if depth == 0.0 {
                (knobs.tap_l, knobs.tap_r)
            } else {
                let (wow_l, wow_r) = spread_apart(wow, knobs.spread);
                let (flutter_l, flutter_r) = if knobs.flutter == 0.0 { (0.0, 0.0) } else { quadrature(flutter) };
                let depth_l = (knobs.tap_l * MOD_DEPTH_FRACTION).min(mod_cap) * depth;
                let echo_r = if steady { knobs.tap_r + share * knobs.tap_l } else { knobs.tap_r };
                let depth_r = (echo_r * MOD_DEPTH_FRACTION).min(mod_cap) * depth;
                (knobs.tap_l + (wow_l + knobs.flutter * flutter_l) * depth_l, knobs.tap_r + (wow_r + knobs.flutter * flutter_r) * depth_r)
            };
            let (wow_at, flutter_at) = (wow, flutter);
            wow = (wow + f64::from(knobs.wow_rate) / sr).fract();
            flutter = (flutter + flutter_step).fract();
            let raw_l = self.left.read(delay_l);
            let raw_r = self.right.read(delay_r);

            let (colored_l, colored_r, hiss_l) = match params.mode {
                DelayMode::Clean => (raw_l, raw_r, 0.0),
                DelayMode::Tape => {
                    self.saturation = self.saturation.at(knobs.drive);
                    (self.left.coloring.tape(raw_l, self.saturation, tape_alpha, dc_pole), self.right.coloring.tape(raw_r, self.saturation, tape_alpha, dc_pole), 0.0)
                }
                DelayMode::Bbd => {
                    let noise = BBD_NOISE_AMP * (raw_l.abs().max(raw_r.abs()) * 10.0 + knobs.feedback_gain).min(1.0);
                    let noise_l = white(&mut rng) as f32 * noise;
                    let noise_r = white(&mut rng) as f32 * noise;
                    (self.left.coloring.bbd(raw_l, knobs.drive, bbd_alpha, noise_l, dc_pole), self.right.coloring.bbd(raw_r, knobs.drive, bbd_alpha, noise_r, dc_pole), noise_l)
                }
            };
            let after = params.cuts_after;
            let toned_l = if after { colored_l } else { self.left.tone(colored_l, &knobs, &paths) };
            let toned_r = if after { colored_r } else { self.right.shared_tone(colored_r, share, &knobs, &paths) };
            let primed = live && self.primed;
            if live {
                self.primed = false;
            }
            let grain_l = self.left.splice(toned_l, period_l, fade, knobs.ratio_l, &sway, bar, downbeat, primed);
            let grain_r = self.right.splice(toned_r, period_r, fade, knobs.ratio_r, &sway, bar, downbeat, primed);
            let echo_l = spliced(toned_l, grain_l, sway.blend);
            let echo_r = spliced(toned_r, grain_r, sway.blend);
            let echo_l = if after { self.left.tone(echo_l, &knobs, &paths) } else { echo_l };
            let echo_r = if after { self.right.shared_tone(echo_r, share, &knobs, &paths) } else { echo_r };
            let mut post_l = self.left.diffuse(echo_l, knobs.diffusion);
            let mut post_r = self.right.diffuse(echo_r, knobs.diffusion);
            let mut sent_l = if steady {
                let colour = |coloring: &mut Coloring, raw: f32| match params.mode {
                    DelayMode::Clean => raw,
                    DelayMode::Tape => coloring.tape(raw, self.saturation, tape_alpha, dc_pole),
                    DelayMode::Bbd => coloring.bbd(raw, knobs.drive, bbd_alpha, hiss_l, dc_pole),
                };
                self.even.steady_echo(self.left.read(knobs.tap_l), self.left.lengths, colour, &knobs, &paths, after, grain_l, sway.blend)
            } else {
                post_l
            };
            if !(post_l.is_finite() && post_r.is_finite() && sent_l.is_finite()) {
                blown = true;
                post_l = finite(post_l);
                post_r = finite(post_r);
                sent_l = finite(sent_l);
                self.left.reset_filters();
                self.right.reset_filters();
                self.even.reset_filters();
            }
            let mut loudest = toned_l.abs().max(toned_r.abs()).max(post_l.abs()).max(post_r.abs()).max(sent_l.abs());

            let back_l = self.left.loop_tone(post_l, &knobs, &paths);
            let back_r = self.right.loop_tone(post_r, &knobs, &paths);
            let (back_l, back_r) = if params.loop_soft { (softened(back_l), softened(back_r)) } else { (back_l, back_r) };
            tickle_sign = -tickle_sign;
            let tickle = tickle_sign * ANTI_DENORM;
            let open = 1.0 - knobs.freeze;
            if ringing || following || lifted {
                guard_env = follow(guard_env, post_l.abs().max(post_r.abs()), guard_attack, guard_release);
                let aim = GUARD_CEILING / guard_env.max(GUARD_CEILING);
                guard = if aim < guard { aim } else { guard + guard_smooth * (aim - guard) };
            } else {
                guard_env = 0.0;
                guard = 1.0;
            }
            let recycle = knobs.feedback_gain * (1.0 - knobs.hold) * open * guard;
            let recycle_r = knobs.feedback_gain_r * (1.0 - knobs.hold) * open * guard;
            let gate = (1.0 - knobs.hold) * open * guard;
            let moved = |gain: f32, amount: f32| if following { (gain + amount * ef * gate).clamp(0.0, FOLLOW_GAIN_MAX * gate) } else { gain };
            let straight = moved(recycle * (1.0 - knobs.ping_pong), knobs.follow_feedback);
            let straight_r = moved(recycle_r * (1.0 - knobs.ping_pong_r), knobs.follow_feedback_r);
            let crossed = moved(recycle * knobs.ping_pong, knobs.follow_cross);
            let crossed_r = moved(recycle_r * knobs.ping_pong_r, knobs.follow_cross_r);
            let kept = knobs.freeze * (1.0 - knobs.ping_pong);
            let kept_r = knobs.freeze * (1.0 - knobs.ping_pong_r);
            let swapped = knobs.freeze * knobs.ping_pong;
            let swapped_r = knobs.freeze * knobs.ping_pong_r;
            let held_l = self.left.frozen(knobs.tap_l);
            let held_r = self.right.frozen(knobs.tap_r);
            let (fed_l, fed_r) = if knobs.lean == 0.0 { (dry_l, dry_r) } else { leaned(dry_l, dry_r, knobs.lean) };
            let (fed_l, fed_r) = (bent(fed_l, knobs.input_drive), bent(fed_r, knobs.input_drive));
            let centred = params.level_c > 0.0 || knobs.centre > CENTRE_FLOOR;
            if !centred && self.centred {
                self.centre.clear();
            }
            self.centred = centred;
            let (centre_l, centre_r) = if centred {
                let delay_c = if depth == 0.0 {
                    knobs.tap_c
                } else {
                    let wow_c = (2.0 * PI * (wow_at + 0.5 * f64::from(knobs.spread))).sin() as f32;
                    let flutter_c = if knobs.flutter == 0.0 { 0.0 } else { quadrature(flutter_at).0 };
                    knobs.tap_c + (wow_c + knobs.flutter * flutter_c) * (knobs.tap_c * MOD_DEPTH_FRACTION).min(mod_cap) * depth
                };
                let raw_c = self.centre.read(delay_c);
                let colored_c = match params.mode {
                    DelayMode::Clean => raw_c,
                    DelayMode::Tape => self.centre.coloring.tape(raw_c, self.saturation, tape_alpha, dc_pole),
                    DelayMode::Bbd => self.centre.coloring.bbd(raw_c, knobs.drive, bbd_alpha, hiss_l, dc_pole),
                };
                let toned_c = if after { colored_c } else { self.centre.tone(colored_c, &knobs, &paths) };
                let grain_c = self.centre.splice(toned_c, period_l, fade, knobs.ratio_l, &sway, bar, downbeat, primed);
                let echo_c = spliced(toned_c, grain_c, sway.blend);
                let echo_c = if after { self.centre.tone(echo_c, &knobs, &paths) } else { echo_c };
                let mut post_c = self.centre.diffuse(echo_c, knobs.diffusion);
                if !post_c.is_finite() {
                    blown = true;
                    post_c = finite(post_c);
                    self.centre.reset_filters();
                }
                let back_c = self.centre.loop_tone(post_c, &knobs, &paths);
                let back_c = if params.loop_soft { softened(back_c) } else { back_c };
                let fed_c = 0.5 * (fed_l + fed_r);
                let straight_c = moved(recycle, knobs.follow_feedback);
                let held_c = self.centre.frozen(knobs.tap_c);
                let wrote_c = finite(open * fed_c + straight_c * back_c + knobs.freeze * held_c + tickle).clamp(-WRITE_CLAMP, WRITE_CLAMP);
                self.centre.ring.push(wrote_c);
                loudest = loudest.max(toned_c.abs()).max(post_c.abs()).max(wrote_c.abs());
                let heard_c = bent(post_c, knobs.wet_drive) * knobs.centre;
                if params.pan_balance { ((1.0 - knobs.pan_c.max(0.0)) * heard_c, (1.0 + knobs.pan_c.min(0.0)) * heard_c) } else { (0.5 * (1.0 - knobs.pan_c) * heard_c, 0.5 * (1.0 + knobs.pan_c) * heard_c) }
            } else {
                (0.0, 0.0)
            };
            let aside = open * share * fed_l;
            let extra = if share > 0.0 { share * (open - recycle) } else { 0.0 };
            let stereo = params.even_stereo && share > 0.0;
            if stereo && !self.stereo {
                self.even.clear_crossing();
            }
            self.stereo = stereo;
            let ((fed_l, fed_r), hop) = if share > 0.0 { (if stereo { (fed_l, fed_r) } else { evened(fed_l, fed_r, share) }, crossed + extra) } else { ((fed_l, fed_r), crossed) };
            let plain = crossed * (1.0 - share);
            let looped = paths[FEEDBACK_FILTER] != Path::Off || params.loop_soft;
            let crossing = if stereo {
                let (through, passed) = self.even.crossing(if steady { knobs.tap_l } else { delay_l }, &knobs, &paths);
                hop * (through + (sent_l - passed))
            } else if steady {
                plain * (if looped { back_l } else { post_l }) + (hop - plain) * sent_l
            } else if looped {
                plain * back_l + (hop - plain) * post_l
            } else {
                hop * post_l
            };
            if stereo {
                self.even.aside.push(aside);
                self.even.first.push(open * fed_l);
            }
            let wrote_l = finite(open * fed_l + straight * back_l + crossed_r * back_r + kept * held_l + swapped_r * held_r + tickle).clamp(-WRITE_CLAMP, WRITE_CLAMP);
            let wrote_r = finite(open * fed_r + straight_r * back_r + crossing + kept_r * held_r + swapped * held_l - tickle).clamp(-WRITE_CLAMP, WRITE_CLAMP);
            self.left.ring.push(wrote_l);
            self.right.ring.push(wrote_r);
            empty = if loudest.max(wrote_l.abs()).max(wrote_r.abs()) > QUIET_FLOOR { 0 } else { empty.saturating_add(1) };

            duck_env = follow(duck_env, key, detect_attack, detect_release);
            duck_active = follow(duck_active, f32::from(u8::from(duck_env > threshold)), duck_attack, duck_release);
            let shut = if knobs.ducking >= 0.0 { duck_active } else { 1.0 - duck_active };
            let duck = 1.0 - knobs.ducking * knobs.ducking * shut;
            let wet_gain = if following { (knobs.wet_gain + knobs.follow_wet * ef).max(0.0) } else { knobs.wet_gain };
            let (heard_l, heard_r) = (bent(post_l, knobs.wet_drive), bent(post_r, knobs.wet_drive));
            let (placed_l, placed_r) = if knobs.pan_l == -1.0 && knobs.pan_r == 1.0 {
                (heard_l, heard_r)
            } else if params.pan_balance {
                let near = |q: f32| (1.0 - q.max(0.0), 1.0 + q.min(0.0));
                let ((left_l, left_r), (right_l, right_r)) = (near(knobs.pan_l), near(knobs.pan_r));
                (left_l * heard_l + right_l * heard_r, left_r * heard_l + right_r * heard_r)
            } else {
                (0.5 * ((1.0 - knobs.pan_l) * heard_l + (1.0 - knobs.pan_r) * heard_r), 0.5 * ((1.0 + knobs.pan_l) * heard_l + (1.0 + knobs.pan_r) * heard_r))
            };
            let (placed_l, placed_r) = if centred { (placed_l + centre_l, placed_r + centre_r) } else { (placed_l, placed_r) };
            let (wet_l, wet_r) = widen(placed_l * duck * wet_gain, placed_r * duck * wet_gain, knobs.width);
            let (wet_l, wet_r) = (wet_l * (1.0 - knobs.pan), wet_r * (1.0 + knobs.pan));
            let dry = 1.0 - knobs.mix + knobs.keep * knobs.mix;
            let dry = if following { (dry + knobs.follow_dry * ef).clamp(0.0, 1.0) } else { dry };
            l[i] = dry * dry_l + knobs.mix * knobs.forward * wet_l;
            r[i] = dry * dry_r + knobs.mix * knobs.forward * wet_r;
            quiet = if key.max(l[i].abs()).max(r[i].abs()) > QUIET_FLOOR { 0 } else { quiet.saturating_add(1) };
        }

        self.knobs = Some(knobs);
        self.wow_phase = wow;
        self.flutter_phase = flutter;
        self.duck_env = duck_env;
        self.duck_active = duck_active;
        self.follow_env = follow_env;
        self.guard_env = guard_env;
        self.guard = guard;
        self.tickle_sign = tickle_sign;
        self.rng = rng;
        self.quiet = quiet;
        self.empty = empty;
        if blown {
            self.clear();
        }
    }
}

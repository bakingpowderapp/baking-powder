use sdk::dsp::{Ring, algebraic, db_to_gain, sanitize, smoothing};
use crate::params::{self, Lookahead};
use crate::{Clock, Unit};

const FAST_RELEASE_SECONDS: f64 = 0.080;
const SLOW_RELEASE_SECONDS: f64 = 1.200;
const SLOW_ATTACK_SECONDS: f64 = 0.100;
const RMS_SECONDS: f64 = 0.010;
const SMOOTH_SECONDS: f64 = 0.010;
const AUTO_MAKEUP_SECONDS: f64 = 2.0;
const AUTO_MAKEUP_MAX_DB: f64 = 24.0;
const AUTO_MAKEUP_GATE_DB: f64 = -60.0;
const MAKEUP_APPLY_SECONDS: f64 = 0.100;
const RING_MIN: usize = 256;
const RING_PAD: usize = 16;
const RATIO_SLOPE_FLOOR: f64 = 1.0001;
const KNEE_HARD_EPS: f64 = 1e-6;
const PEAK_EPS: f64 = 1e-10;
const RMS_EPS: f64 = 1e-20;
const GR_FASTPATH_DB: f64 = 0.01;
const DC_TICKLE: f64 = 1e-25;
const SOFT_CLIP_PRE: f32 = 1.2589;
const SOFT_CLIP_POST: f32 = 0.7943;

pub(crate) fn gain_reduction(level_db: f64, threshold_db: f64, slope: f64, knee_db: f64) -> f64 {
    if knee_db < KNEE_HARD_EPS {
        return if level_db <= threshold_db { 0.0 } else { (level_db - threshold_db) * slope };
    }
    let half = 0.5 * knee_db;
    if level_db <= threshold_db - half {
        0.0
    } else if level_db > threshold_db + half {
        (level_db - threshold_db) * slope
    } else {
        let d = level_db - threshold_db + half;
        slope * d * d / (2.0 * knee_db)
    }
}

pub(crate) fn ring_frames(sample_rate: f64) -> usize {
    (Lookahead::Ms5.samples(sample_rate) + RING_PAD).max(RING_MIN)
}

pub(crate) fn sounding(ring_l: &Ring, ring_r: &Ring, frames: usize) -> bool {
    let gate = db_to_gain(AUTO_MAKEUP_GATE_DB) as f32;
    (1..=frames).any(|back| ring_l.tap(back).abs() > gate || ring_r.tap(back).abs() > gate)
}

pub(crate) fn slope_of(ratio: f64) -> f64 {
    1.0 - 1.0 / ratio.max(RATIO_SLOPE_FLOOR)
}

fn soft_clip(x: f32) -> f32 {
    SOFT_CLIP_POST * algebraic(f64::from(x * SOFT_CLIP_PRE)) as f32
}

#[derive(Clone, Copy)]
struct Control {
    primed: bool,
    auto_was_on: bool,
    tickle: f64,
    mean_square: f64,
    held: f64,
    fast: f64,
    slow: f64,
    threshold_db: f64,
    slope: f64,
    knee_db: f64,
    range_db: f64,
    mix: f64,
    makeup_db: f64,
    auto_makeup_db: f64,
    gated: f64,
}

impl Control {
    fn fresh() -> Control {
        let d = params::Compressor::DEFAULT;
        Control {
            primed: false,
            auto_was_on: d.auto,
            tickle: DC_TICKLE,
            mean_square: 0.0,
            held: 0.0,
            fast: 0.0,
            slow: 0.0,
            threshold_db: d.threshold_db,
            slope: slope_of(d.ratio),
            knee_db: d.knee_db,
            range_db: d.range_db,
            mix: 1.0,
            makeup_db: if d.auto { 0.0 } else { d.makeup_db },
            auto_makeup_db: 0.0,
            gated: 0.0,
        }
    }
}

pub struct Compressor {
    sample_rate: f64,
    ring_l: Ring,
    ring_r: Ring,
    control: Control,
    gr_meter: f32,
}

impl Compressor {
    fn sounding(&self) -> bool {
        sounding(&self.ring_l, &self.ring_r, ring_frames(self.sample_rate))
    }
}

impl Unit for Compressor {
    type Params = params::Compressor;

    fn new(sample_rate: f64, _: usize) -> Compressor {
        let ring = ring_frames(sample_rate);
        Compressor { sample_rate, ring_l: Ring::new(ring), ring_r: Ring::new(ring), control: Control::fresh(), gr_meter: 0.0 }
    }

    fn restart(&mut self) {
        if self.sounding() {
            self.control.gated = 0.0;
        } else {
            self.control = Control::fresh();
        }
    }

    fn gauge(&self) -> f32 {
        self.gr_meter
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], params: &params::Compressor, wet: f32, _: &Clock) {
        let n = l.len().min(r.len());
        let (l, r) = (&mut l[..n], &mut r[..n]);
        sanitize(l, r);
        let sr = self.sample_rate;
        let pole = |seconds: f64| (-1.0 / (seconds * sr)).exp();
        let attack = pole(params.attack_ms * 0.001);
        let release = if params.auto { pole(FAST_RELEASE_SECONDS) } else { pole(params.release_ms * 0.001) };
        let slow_attack = pole(SLOW_ATTACK_SECONDS);
        let slow_release = pole(SLOW_RELEASE_SECONDS);
        let rms_smooth = smoothing(RMS_SECONDS, sr);
        let smooth = smoothing(SMOOTH_SECONDS, sr);
        let makeup_track = smoothing(AUTO_MAKEUP_SECONDS, sr);
        let makeup_apply = smoothing(MAKEUP_APPLY_SECONDS, sr);
        let slope_target = slope_of(params.ratio);
        let mix_target = f64::from(wet);
        let Control { primed, auto_was_on, mut tickle, mut mean_square, mut held, mut fast, mut slow, mut threshold_db, mut slope, mut knee_db, mut range_db, mut mix, mut makeup_db, mut auto_makeup_db, mut gated } = self.control;
        if !primed {
            threshold_db = params.threshold_db;
            slope = slope_target;
            knee_db = params.knee_db;
            range_db = params.range_db;
            mix = mix_target;
            makeup_db = if params.auto { auto_makeup_db } else { params.makeup_db };
        }
        if params.auto && !auto_was_on {
            slow = fast;
        }
        let lookahead = params.lookahead.samples(sr);
        let mut block_max_db = 0.0f64;
        for i in 0..n {
            threshold_db += smooth * (params.threshold_db - threshold_db);
            slope += smooth * (slope_target - slope);
            knee_db += smooth * (params.knee_db - knee_db);
            range_db += smooth * (params.range_db - range_db);
            mix += smooth * (mix_target - mix);

            let (dry_l, dry_r) = (l[i], r[i]);
            self.ring_l.push(dry_l);
            self.ring_r.push(dry_r);
            let (look_l, look_r) = (self.ring_l.tap(lookahead + 1), self.ring_r.tap(lookahead + 1));

            tickle = -tickle;
            let key_l = f64::from(dry_l) + tickle;
            let key_r = f64::from(dry_r) + tickle;
            let detected = if params.sum { 0.5 * (key_l + key_r).abs() } else { key_l.abs().max(key_r.abs()) };
            let level_db = if params.peak {
                20.0 * (detected + PEAK_EPS).log10()
            } else {
                mean_square = (1.0 - rms_smooth) * mean_square + rms_smooth * detected * detected;
                10.0 * (mean_square + RMS_EPS).log10()
            };

            let gr = gain_reduction(level_db, threshold_db, slope, knee_db);
            if params.branching {
                fast = if gr > fast { attack * fast + (1.0 - attack) * gr } else { release * fast + (1.0 - release) * gr };
                held = fast;
            } else {
                held = gr.max(release * held + (1.0 - release) * gr);
                fast = attack * fast + (1.0 - attack) * held;
            }
            let envelope = if params.auto {
                slow = if gr > slow { slow_attack * slow + (1.0 - slow_attack) * gr } else { slow_release * slow + (1.0 - slow_release) * gr };
                fast.max(slow)
            } else {
                fast
            };
            let reduction_db = envelope.min(range_db);
            block_max_db = block_max_db.max(reduction_db);
            let gain = if reduction_db < GR_FASTPATH_DB { 1.0 } else { db_to_gain(-reduction_db) as f32 };

            if level_db > AUTO_MAKEUP_GATE_DB {
                gated += 1.0;
                auto_makeup_db += makeup_track.max(1.0 / gated) * (reduction_db - auto_makeup_db);
                auto_makeup_db = auto_makeup_db.min(AUTO_MAKEUP_MAX_DB);
            }
            let makeup_target = if params.auto { auto_makeup_db } else { params.makeup_db };
            makeup_db += makeup_apply * (makeup_target - makeup_db);
            let makeup = db_to_gain(makeup_db) as f32;

            let (mut wet_l, mut wet_r) = (look_l * gain * makeup, look_r * gain * makeup);
            if params.soft_clip {
                wet_l = soft_clip(wet_l);
                wet_r = soft_clip(wet_r);
            }
            let wet_mix = mix as f32;
            let dry_mix = 1.0 - wet_mix + params.dry as f32 * wet_mix;
            l[i] = dry_mix * look_l + wet_mix * wet_l;
            r[i] = dry_mix * look_r + wet_mix * wet_r;
        }
        self.control = Control { primed: true, auto_was_on: params.auto, tickle, mean_square, held, fast, slow, threshold_db, slope, knee_db, range_db, mix, makeup_db, auto_makeup_db, gated };
        self.gr_meter = block_max_db as f32;
    }
}

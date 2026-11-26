use sdk::dsp::{Ring, db_to_gain, finite, sanitize, smoothing};
use crate::compressor::{ring_frames, sounding};
use crate::params;
use crate::{Clock, Unit};

const SMOOTH_SECONDS: f64 = 0.010;
const PEAK_EPS: f64 = 1e-10;
const SHUT_DB: f64 = 80.0;

fn floor_gain(floor_db: f64) -> f64 {
    if floor_db <= -SHUT_DB { 0.0 } else { db_to_gain(floor_db) }
}

pub struct Gate {
    sample_rate: f64,
    signal_l: Vec<f32>,
    signal_r: Vec<f32>,
    fed: usize,
    read: usize,
    ring_l: Ring,
    ring_r: Ring,
    open: bool,
    held: usize,
    gain: Option<f64>,
    threshold_db: Option<f64>,
    floor: Option<f64>,
    mix: Option<f64>,
    level_meter: f32,
    cut_meter: f32,
}

impl Unit for Gate {
    type Params = params::Gate;

    fn new(sample_rate: f64, max_frames: usize) -> Gate {
        let ring = ring_frames(sample_rate);
        Gate { sample_rate, signal_l: vec![0.0; max_frames], signal_r: vec![0.0; max_frames], fed: 0, read: 0, ring_l: Ring::new(ring), ring_r: Ring::new(ring), open: false, held: 0, gain: None, threshold_db: None, floor: None, mix: None, level_meter: f32::NEG_INFINITY, cut_meter: 0.0 }
    }

    fn restart(&mut self) {
        if !sounding(&self.ring_l, &self.ring_r, ring_frames(self.sample_rate)) {
            (self.open, self.held, self.gain, self.threshold_db, self.floor, self.mix) = (false, 0, None, None, None, None);
        }
    }

    fn feed(&mut self, l: &[f32], r: &[f32]) {
        let n = l.len().min(r.len()).min(self.signal_l.len());
        self.signal_l[..n].copy_from_slice(&l[..n]);
        self.signal_r[..n].copy_from_slice(&r[..n]);
        self.fed = n;
        self.read = 0;
    }

    fn starve(&mut self) {
        self.fed = 0;
        self.read = 0;
    }

    fn gauge(&self) -> f32 {
        self.cut_meter
    }

    fn signal(&self) -> f32 {
        self.level_meter
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], params: &params::Gate, wet: f32, _: &Clock) {
        let n = l.len().min(r.len());
        let (l, r) = (&mut l[..n], &mut r[..n]);
        sanitize(l, r);
        let sr = self.sample_rate;
        let attack = smoothing(params.attack_ms * 0.001, sr);
        let release = smoothing(params.release_ms * 0.001, sr);
        let smooth = smoothing(SMOOTH_SECONDS, sr);
        let hold = (params.hold_ms * 0.001 * sr).round() as usize;
        let floor_target = floor_gain(params.floor_db);
        let mix_target = f64::from(wet);
        let lookahead = params.lookahead.samples(sr);
        let (mut threshold_db, mut floor, mut mix) = (self.threshold_db.unwrap_or(params.threshold_db), self.floor.unwrap_or(floor_target), self.mix.unwrap_or(mix_target));
        let mut gain = self.gain.unwrap_or(if params.flip { 1.0 } else { floor });
        let (mut open, mut held) = (self.open, self.held);
        let (signalled, fed, read) = (params.signalled(), self.fed, self.read);
        let mut block_level_db = f64::NEG_INFINITY;
        let mut block_cut_db = 0.0f64;
        for i in 0..n {
            threshold_db += smooth * (params.threshold_db - threshold_db);
            floor += smooth * (floor_target - floor);
            mix += smooth * (mix_target - mix);
            let (dry_l, dry_r) = (l[i], r[i]);
            self.ring_l.push(dry_l);
            self.ring_r.push(dry_r);
            let (look_l, look_r) = (self.ring_l.tap(lookahead + 1), self.ring_r.tap(lookahead + 1));
            let heard = if !signalled {
                dry_l.abs().max(dry_r.abs())
            } else if read + i < fed {
                finite(self.signal_l[read + i]).abs().max(finite(self.signal_r[read + i]).abs())
            } else {
                0.0
            };
            let level_db = 20.0 * (f64::from(heard) + PEAK_EPS).log10();
            block_level_db = block_level_db.max(level_db);
            if level_db > threshold_db {
                open = true;
                held = hold;
            } else if open && level_db < threshold_db - params.return_db {
                if held > 0 {
                    held -= 1;
                } else {
                    open = false;
                }
            }
            let aim = if open != params.flip { 1.0 } else { floor };
            gain += if aim > gain { attack } else { release } * (aim - gain);
            block_cut_db = block_cut_db.max(-20.0 * (gain + PEAK_EPS).log10());
            let blend = ((1.0 - mix) + mix * gain) as f32;
            l[i] = look_l * blend;
            r[i] = look_r * blend;
        }
        self.threshold_db = Some(threshold_db);
        self.floor = Some(floor);
        self.mix = Some(mix);
        self.gain = Some(gain);
        self.open = open;
        self.held = held;
        self.read = (read + n).min(fed);
        self.level_meter = block_level_db as f32;
        self.cut_meter = block_cut_db.min(SHUT_DB) as f32;
    }
}

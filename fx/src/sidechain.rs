use crate::compressor::{gain_reduction, ring_frames, slope_of, sounding};
use sdk::dsp::{Greatest, Ring, db_to_gain, sanitize, smoothing};
use crate::params;
use crate::{Clock, Unit};

const RMS_SECONDS: f64 = 0.010;
const SMOOTH_SECONDS: f64 = 0.010;
const PEAK_EPS: f64 = 1e-10;
const RMS_EPS: f64 = 1e-20;
const GR_FASTPATH_DB: f64 = 0.01;

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Heard {
    pub peak: f32,
    pub power: f32,
    pub levels_db: [f32; Heard::PARTS],
}

impl Heard {
    pub const PARTS: usize = 4;
    pub const QUIET_DB: f32 = -100.0;
    pub const QUIET: Heard = Heard { peak: 0.0, power: 0.0, levels_db: [Heard::QUIET_DB; Heard::PARTS] };
}

#[derive(Clone, Copy)]
struct Pace {
    attack: f64,
    release: f64,
    rms: f64,
    hold: u64,
}

impl Pace {
    fn of(params: &params::Sidechain, rate: f64) -> Pace {
        let pole = |seconds: f64| (-1.0 / (seconds * rate)).exp();
        Pace { attack: pole(params.attack_ms * 0.001), release: pole(params.release_ms * 0.001), rms: smoothing(RMS_SECONDS, rate), hold: (params.hold_ms * 0.001 * rate).round() as u64 }
    }
}

struct Follower {
    mean_square: f64,
    held: f64,
    hold: Greatest,
    fast: f64,
}

impl Follower {
    fn new(rate: f64) -> Follower {
        Follower { mean_square: 0.0, held: 0.0, hold: Greatest::new((params::Sidechain::MAX_HOLD_MS * 0.001 * rate).ceil() as usize + 1), fast: 0.0 }
    }

    fn level_db(&mut self, signal: f64, peaks: bool, pace: &Pace) -> f64 {
        if peaks {
            return peak_db(signal);
        }
        self.mean_square = (1.0 - pace.rms) * self.mean_square + pace.rms * signal * signal;
        self.mean_db()
    }

    fn mean_db(&self) -> f64 {
        10.0 * (self.mean_square + RMS_EPS).log10()
    }

    fn reduce(&mut self, reduction_db: f64, range_db: f64, pace: &Pace) -> f64 {
        self.held = reduction_db.max(pace.release * self.held + (1.0 - pace.release) * reduction_db);
        self.fast = pace.attack * self.fast + (1.0 - pace.attack) * self.hold.greatest(self.held, pace.hold);
        self.within(range_db)
    }

    fn within(&self, range_db: f64) -> f64 {
        if range_db < params::Sidechain::UNBOUND_DB { self.fast.min(range_db) } else { self.fast }
    }

    fn rest(&mut self) {
        (self.mean_square, self.held, self.fast) = (0.0, 0.0, 0.0);
        self.hold.clear();
    }
}

fn peak_db(signal: f64) -> f64 {
    20.0 * (signal + PEAK_EPS).log10()
}

fn gain_of(reduction_db: f64) -> f64 {
    if reduction_db < GR_FASTPATH_DB { 1.0 } else { db_to_gain(-reduction_db) }
}

pub fn duck(heard: impl Iterator<Item = (Heard, params::Sidechain, f64)>, rate: f64) -> Vec<f32> {
    let mut follower = Follower::new(rate);
    let smooth = smoothing(SMOOTH_SECONDS, rate);
    let mut timed: Option<([f64; 3], (Pace, f64, f64))> = None;
    let mut glided: Option<[f64; 5]> = None;
    let mut before = 0.0;
    let gains = heard.map(|(heard, params, wet)| {
        let times = [params.attack_ms, params.hold_ms, params.release_ms];
        let (pace, midway, leaning) = match timed {
            Some((held, paced)) if held == times => paced,
            _ => {
                let pace = Pace::of(&params, rate);
                let paced = (pace, pace.release.sqrt(), 1.0 - (1.0 - pace.attack) / -pace.attack.ln());
                timed = Some((times, paced));
                paced
            }
        };
        let aimed = [params.threshold_db, slope_of(params.ratio), params.knee_db, params.range(), wet];
        let glide = glided.get_or_insert(aimed);
        for (at, aim) in glide.iter_mut().zip(aimed) {
            *at += smooth * (aim - *at);
        }
        let [threshold_db, slope, knee_db, range_db, mix] = *glide;
        let reduced = |level_db: f64| gain_reduction(level_db, threshold_db, slope, knee_db);
        let (top, mean) = if params.peak {
            let mean = heard.levels_db.iter().map(|level_db| reduced(f64::from(*level_db))).sum::<f64>() / Heard::PARTS as f64;
            let top = reduced(peak_db(f64::from(heard.peak)));
            (top - (1.0 - midway) * (top - mean), mean)
        } else {
            follower.mean_square = (1.0 - pace.rms) * follower.mean_square + pace.rms * f64::from(heard.power);
            let even = reduced(follower.mean_db());
            (even, even)
        };
        follower.held = top.max(pace.release * follower.held + (1.0 - pace.release) * mean);
        let held = follower.hold.greatest(follower.held, pace.hold);
        follower.fast = pace.attack * follower.fast + (1.0 - pace.attack - leaning) * before + leaning * held;
        before = held;
        ((1.0 - mix) + mix * gain_of(follower.within(range_db))) as f32
    });
    gains.collect()
}

pub struct Sidechain {
    sample_rate: f64,
    signal_l: Vec<f32>,
    signal_r: Vec<f32>,
    fed: usize,
    read: usize,
    ring_l: Ring,
    ring_r: Ring,
    follower: Follower,
    threshold_db: f64,
    slope: f64,
    knee_db: f64,
    range_db: f64,
    settled: bool,
    mix: Option<f64>,
    gr_meter: f32,
    signal_meter: f32,
}

impl Unit for Sidechain {
    type Params = params::Sidechain;

    fn new(sample_rate: f64, max_frames: usize) -> Sidechain {
        let d = params::Sidechain::DEFAULT;
        let ring = ring_frames(sample_rate);
        Sidechain {
            sample_rate,
            signal_l: vec![0.0; max_frames],
            signal_r: vec![0.0; max_frames],
            fed: 0,
            read: 0,
            ring_l: Ring::new(ring),
            ring_r: Ring::new(ring),
            follower: Follower::new(sample_rate),
            threshold_db: d.threshold_db,
            slope: slope_of(d.ratio),
            knee_db: d.knee_db,
            range_db: d.range(),
            settled: false,
            mix: None,
            gr_meter: 0.0,
            signal_meter: f32::NEG_INFINITY,
        }
    }

    fn restart(&mut self) {
        if !sounding(&self.ring_l, &self.ring_r, ring_frames(self.sample_rate)) {
            self.follower.rest();
            (self.settled, self.mix) = (false, None);
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
        self.gr_meter
    }

    fn signal(&self) -> f32 {
        self.signal_meter
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], params: &params::Sidechain, wet: f32, _: &Clock) {
        let n = l.len().min(r.len());
        let (l, r) = (&mut l[..n], &mut r[..n]);
        sanitize(l, r);
        let sr = self.sample_rate;
        let pace = Pace::of(params, sr);
        let smooth = smoothing(SMOOTH_SECONDS, sr);
        let slope_target = slope_of(params.ratio);
        let mix_target = f64::from(wet);
        let lookahead = params.lookahead.samples(sr);
        let (mut threshold_db, mut slope, mut knee_db, mut range_db) = if self.settled { (self.threshold_db, self.slope, self.knee_db, self.range_db) } else { (params.threshold_db, slope_target, params.knee_db, params.range()) };
        let mut mix = self.mix.unwrap_or(mix_target);
        let (fed, read) = (self.fed, self.read);
        let mut block_max_db = 0.0f64;
        let mut block_signal_db = f64::NEG_INFINITY;
        for i in 0..n {
            threshold_db += smooth * (params.threshold_db - threshold_db);
            slope += smooth * (slope_target - slope);
            knee_db += smooth * (params.knee_db - knee_db);
            range_db += smooth * (params.range() - range_db);
            mix += smooth * (mix_target - mix);
            self.ring_l.push(l[i]);
            self.ring_r.push(r[i]);
            let (look_l, look_r) = (self.ring_l.tap(lookahead + 1), self.ring_r.tap(lookahead + 1));
            let heard = if read + i < fed { f64::from(self.signal_l[read + i].abs().max(self.signal_r[read + i].abs())) } else { 0.0 };
            let signal = if heard.is_finite() { heard } else { 0.0 };
            let level_db = self.follower.level_db(signal, params.peak, &pace);
            block_signal_db = block_signal_db.max(level_db);
            let reduction_db = self.follower.reduce(gain_reduction(level_db, threshold_db, slope, knee_db), range_db, &pace);
            block_max_db = block_max_db.max(reduction_db);
            let gain = gain_of(reduction_db) as f32;
            let wet_mix = mix as f32;
            let blend = (1.0 - wet_mix) + wet_mix * gain;
            l[i] = look_l * blend;
            r[i] = look_r * blend;
        }
        self.threshold_db = threshold_db;
        self.slope = slope;
        self.knee_db = knee_db;
        self.range_db = range_db;
        self.settled = true;
        self.mix = Some(mix);
        self.read = (read + n).min(fed);
        self.gr_meter = block_max_db as f32;
        self.signal_meter = block_signal_db as f32;
    }
}

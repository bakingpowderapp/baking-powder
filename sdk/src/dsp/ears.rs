use super::db_to_gain;
use super::fft::Fft;
use super::svf::{Biquad, BiquadCoeffs};
use std::f64::consts::PI;

pub const BANDS: [(&str, f64, f64); 5] = [("sub", 40.0, 120.0), ("lowMid", 120.0, 400.0), ("mid", 400.0, 1600.0), ("high", 1600.0, 6400.0), ("top", 6400.0, 16000.0)];
pub const FLOOR_DB: f64 = -120.0;
pub const HOP_SECONDS: f64 = 0.01;

const WHOLE_HZ: (f64, f64) = (20.0, 16000.0);
const WINDOW: usize = 8192;
const SPARSE_WINDOWS: usize = 8;
const OVERSAMPLE: usize = 4;
const TAPS: usize = 12;
const BLOCK_HOPS: usize = 40;
const BLOCK_STEP: usize = 10;
const ONSET_RISE_DB: f64 = 6.0;
const ONSET_FLOOR_DB: f64 = -60.0;
const ONSET_LOOK_HOPS: usize = 3;
const ONSET_HOLD_HOPS: usize = 5;
const NO_ONSET: u32 = u32::MAX;
const ABSOLUTE_GATE_LUFS: f64 = -70.0;
const RELATIVE_GATE_LU: f64 = -10.0;
const LOUDNESS_OFFSET_DB: f64 = -0.691;
const SHELF_HZ: f64 = 1681.974450955533;
const SHELF_DB: f64 = 3.999843853973347;
const SHELF_Q: f64 = 0.7071752369554196;
const SHELF_SPREAD: f64 = 0.4996667741545416;
const RUMBLE_HZ: f64 = 38.13547087602444;
const RUMBLE_Q: f64 = 0.5003270373238773;

pub struct Heard {
    pub seconds: f64,
    pub lufs: Option<f64>,
    pub peak_db: f64,
    pub true_peak_db: f64,
    pub rms_db: f64,
    pub correlation: Option<f64>,
    pub side_db: f64,
    pub bands_db: Option<[f64; BANDS.len()]>,
    pub bars: Vec<(f64, f64)>,
    rate: f64,
    hop: usize,
    hops: Vec<f64>,
    hop_peaks: Vec<f32>,
    hop_peak_frames: Vec<u32>,
    hop_onsets: Vec<u32>,
    hop_powers: Vec<f32>,
    hop_slopes: Vec<f32>,
}

pub struct Hit {
    pub seconds: f64,
    pub peak: f32,
    pub hz: f64,
}

impl Heard {
    pub fn between(&self, from_seconds: f64, until_seconds: f64) -> (f64, f64) {
        let (from, until) = self.hops_between(from_seconds, until_seconds);
        let peak = self.hop_peaks[from..until].iter().copied().fold(0.0, f32::max);
        (integrated(&self.hops[from..until]).unwrap_or(FLOOR_DB), amplitude_db(peak))
    }

    pub fn peak_at(&self, from_seconds: f64, until_seconds: f64) -> Option<f64> {
        let (from, until) = self.hops_between(from_seconds, until_seconds);
        let loudest = (from..until).max_by(|a, b| self.hop_peaks[*a].total_cmp(&self.hop_peaks[*b]))?;
        (self.hop_peaks[loudest] > 0.0).then(|| self.instant(loudest))
    }

    pub fn hits(&self, from_seconds: f64, until_seconds: f64) -> Vec<Hit> {
        let (from, until) = self.hops_between(from_seconds, until_seconds);
        let until = until.min(self.hop_onsets.len());
        let mut out = Vec::new();
        let mut hop = from;
        while hop < until {
            if self.hop_onsets[hop] == NO_ONSET {
                hop += 1;
                continue;
            }
            let held = hop..(hop + ONSET_HOLD_HOPS).min(until);
            let summed = |hops: &[f32]| hops[held.clone()].iter().copied().map(f64::from).sum::<f64>();
            let peak = self.hop_peaks[held.clone()].iter().copied().fold(0.0, f32::max);
            let turn = (summed(&self.hop_slopes) / (4.0 * summed(&self.hop_powers))).sqrt().min(1.0).asin();
            out.push(Hit { seconds: (hop * self.hop + self.hop_onsets[hop] as usize) as f64 / self.rate, peak, hz: self.rate * turn / PI });
            hop = held.end;
        }
        out
    }

    fn hops_between(&self, from_seconds: f64, until_seconds: f64) -> (usize, usize) {
        let hop = |seconds: f64| hop_of(seconds).min(self.hops.len());
        let from = hop(from_seconds);
        (from, hop(until_seconds).max((from + 1).min(self.hops.len())))
    }

    fn instant(&self, hop: usize) -> f64 {
        (hop * self.hop + self.hop_peak_frames[hop] as usize) as f64 / self.rate
    }
}

pub fn hop_of(seconds: f64) -> usize {
    (seconds / HOP_SECONDS).round().max(0.0) as usize
}

struct Interpolator {
    phases: [[f64; TAPS]; OVERSAMPLE - 1],
    history: [[f64; TAPS]; 2],
    at: usize,
    peak: f64,
}

impl Interpolator {
    fn new() -> Interpolator {
        let half = (TAPS / 2) as f64;
        let phases = std::array::from_fn(|phase| {
            let offset = (phase + 1) as f64 / OVERSAMPLE as f64;
            let mut taps: [f64; TAPS] = std::array::from_fn(|tap| {
                let t = tap as f64 - (half - 1.0) - offset;
                if t.abs() >= half {
                    return 0.0;
                }
                let window = 0.42 + 0.5 * (PI * t / half).cos() + 0.08 * (2.0 * PI * t / half).cos();
                (PI * t).sin() / (PI * t) * window
            });
            let sum: f64 = taps.iter().sum();
            for tap in &mut taps {
                *tap /= sum;
            }
            taps
        });
        Interpolator { phases, history: [[0.0; TAPS]; 2], at: 0, peak: 0.0 }
    }

    fn push(&mut self, left: f64, right: f64) {
        self.history[0][self.at] = left;
        self.history[1][self.at] = right;
        self.at = (self.at + 1) % TAPS;
        for phase in &self.phases {
            for channel in &self.history {
                let between: f64 = phase.iter().enumerate().map(|(tap, coefficient)| coefficient * channel[(self.at + tap) % TAPS]).sum();
                self.peak = self.peak.max(between.abs());
            }
        }
    }
}

struct Strikes {
    floor: f32,
    rise: f32,
    clears: f32,
    last: (f64, f64),
    onset: u32,
    power: f64,
    slope: f64,
    onsets: Vec<u32>,
    powers: Vec<f32>,
    slopes: Vec<f32>,
}

impl Strikes {
    fn new() -> Strikes {
        let floor = db_to_gain(ONSET_FLOOR_DB) as f32;
        Strikes { floor, rise: db_to_gain(ONSET_RISE_DB) as f32, clears: floor, last: (0.0, 0.0), onset: NO_ONSET, power: 0.0, slope: 0.0, onsets: Vec::new(), powers: Vec::new(), slopes: Vec::new() }
    }

    fn hear(&mut self, x: f64, y: f64, peak: f32, frame: usize) {
        if self.onset == NO_ONSET && peak >= self.clears {
            self.onset = frame as u32;
        }
        let (dx, dy) = (x - self.last.0, y - self.last.1);
        self.power += x * x + y * y;
        self.slope += dx * dx + dy * dy;
        self.last = (x, y);
    }

    fn close(&mut self, peaks: &[f32]) {
        self.onsets.push(self.onset);
        self.powers.push(self.power as f32);
        self.slopes.push(self.slope as f32);
        let before = peaks[peaks.len().saturating_sub(ONSET_LOOK_HOPS)..].iter().copied().fold(0.0, f32::max);
        self.clears = (before * self.rise).max(self.floor);
        (self.onset, self.power, self.slope) = (NO_ONSET, 0.0, 0.0);
    }
}

pub struct Ear {
    rate: f64,
    bar: usize,
    weigh: [[Biquad; 2]; 2],
    interpolator: Option<Interpolator>,
    strikes: Option<Strikes>,
    hop: usize,
    hop_power: f64,
    hop_peak: f32,
    hop_peak_at: usize,
    hop_frames: usize,
    hops: Vec<f64>,
    hop_peaks: Vec<f32>,
    hop_peak_frames: Vec<u32>,
    every: usize,
    windows: usize,
    bar_power: f64,
    bar_peak: f32,
    bar_frames: usize,
    bars: Vec<(f64, f64)>,
    frames: usize,
    peak: f32,
    left: f64,
    right: f64,
    both: f64,
    mid: f64,
    side: f64,
    fft: Fft,
    window: Vec<f64>,
    held: Vec<f64>,
    re: Vec<f64>,
    im: Vec<f64>,
    band_bins: [(usize, usize); BANDS.len()],
    whole_bins: (usize, usize),
    band_power: [f64; BANDS.len()],
    whole_power: f64,
}

impl Ear {
    pub fn new(rate: f64, bar_frames: usize) -> Ear {
        let mut ear = Ear::spaced(rate, bar_frames, 1);
        ear.interpolator = Some(Interpolator::new());
        ear.strikes = Some(Strikes::new());
        ear
    }

    pub fn sparse(rate: f64, bar_frames: usize) -> Ear {
        Ear::spaced(rate, bar_frames, SPARSE_WINDOWS)
    }

    fn spaced(rate: f64, bar_frames: usize, every: usize) -> Ear {
        let bin = |hz: f64| ((hz * WINDOW as f64 / rate).ceil() as usize).min(WINDOW / 2);
        let weighting = weighting(rate);
        Ear {
            rate,
            bar: bar_frames.max(1),
            weigh: [weighting.map(Biquad::with), weighting.map(Biquad::with)],
            interpolator: None,
            strikes: None,
            hop: ((rate * HOP_SECONDS).round() as usize).max(1),
            hop_power: 0.0,
            hop_peak: 0.0,
            hop_peak_at: 0,
            hop_frames: 0,
            hops: Vec::new(),
            hop_peaks: Vec::new(),
            hop_peak_frames: Vec::new(),
            every,
            windows: 0,
            bar_power: 0.0,
            bar_peak: 0.0,
            bar_frames: 0,
            bars: Vec::new(),
            frames: 0,
            peak: 0.0,
            left: 0.0,
            right: 0.0,
            both: 0.0,
            mid: 0.0,
            side: 0.0,
            fft: Fft::new(WINDOW),
            window: (0..WINDOW).map(|i| 0.5 - 0.5 * (2.0 * PI * i as f64 / WINDOW as f64).cos()).collect(),
            held: Vec::with_capacity(WINDOW),
            re: vec![0.0; WINDOW],
            im: vec![0.0; WINDOW],
            band_bins: BANDS.map(|(_, low, high)| (bin(low), bin(high))),
            whole_bins: (bin(WHOLE_HZ.0), bin(WHOLE_HZ.1)),
            band_power: [0.0; BANDS.len()],
            whole_power: 0.0,
        }
    }

    pub fn hear(&mut self, left: &[f32], right: &[f32]) {
        for (&l, &r) in left.iter().zip(right) {
            let (x, y) = (f64::from(l), f64::from(r));
            let (weighed_x, weighed_y) = (weighed(&mut self.weigh[0], x), weighed(&mut self.weigh[1], y));
            let power = weighed_x * weighed_x + weighed_y * weighed_y;
            let peak = l.abs().max(r.abs());
            if let Some(interpolator) = &mut self.interpolator {
                interpolator.push(x, y);
            }
            if let Some(strikes) = &mut self.strikes {
                strikes.hear(x, y, peak, self.hop_frames);
            }
            self.hop_power += power;
            if peak > self.hop_peak {
                self.hop_peak = peak;
                self.hop_peak_at = self.hop_frames;
            }
            self.hop_frames += 1;
            if self.hop_frames == self.hop {
                self.close_hop();
            }
            self.bar_power += power;
            self.bar_peak = self.bar_peak.max(peak);
            self.bar_frames += 1;
            if self.bar_frames == self.bar {
                self.close_bar();
            }
            self.peak = self.peak.max(peak);
            self.left += x * x;
            self.right += y * y;
            self.both += x * y;
            let (mid, side) = (0.5 * (x + y), 0.5 * (x - y));
            self.mid += mid * mid;
            self.side += side * side;
            self.held.push(mid);
            if self.held.len() == WINDOW {
                if self.windows.is_multiple_of(self.every) {
                    self.spectrum(WINDOW);
                }
                self.windows += 1;
                self.held.drain(..WINDOW / 2);
            }
            self.frames += 1;
        }
    }

    pub fn finish(mut self) -> Heard {
        if self.hop_frames > 0 {
            self.close_hop();
        }
        if self.bar_frames > 0 {
            self.close_bar();
        }
        if self.windows == 0 && !self.held.is_empty() {
            let length = self.held.len();
            self.spectrum(length);
        }
        let channel_power = (self.left + self.right) / (2 * self.frames.max(1)) as f64;
        let correlation = (self.left * self.right > 0.0).then(|| self.both / (self.left * self.right).sqrt());
        let bands_db = (self.whole_power > 0.0).then(|| self.band_power.map(|power| power_db(power / self.whole_power)));
        let true_peak = self.interpolator.as_ref().map_or(f64::from(self.peak), |interpolator| interpolator.peak.max(f64::from(self.peak)));
        let (hop_onsets, hop_powers, hop_slopes) = self.strikes.map_or_else(Default::default, |strikes| (strikes.onsets, strikes.powers, strikes.slopes));
        Heard {
            seconds: self.frames as f64 / self.rate,
            lufs: integrated(&self.hops),
            peak_db: amplitude_db(self.peak),
            true_peak_db: amplitude_db(true_peak as f32),
            rms_db: power_db(channel_power),
            correlation,
            side_db: if self.mid > 0.0 { power_db(self.side / self.mid) } else { FLOOR_DB },
            bands_db,
            bars: self.bars,
            rate: self.rate,
            hop: self.hop,
            hops: self.hops,
            hop_peaks: self.hop_peaks,
            hop_peak_frames: self.hop_peak_frames,
            hop_onsets,
            hop_powers,
            hop_slopes,
        }
    }

    fn close_hop(&mut self) {
        self.hops.push(self.hop_power / self.hop_frames as f64);
        self.hop_peaks.push(self.hop_peak);
        self.hop_peak_frames.push(self.hop_peak_at as u32);
        if let Some(strikes) = &mut self.strikes {
            strikes.close(&self.hop_peaks);
        }
        (self.hop_power, self.hop_peak, self.hop_peak_at, self.hop_frames) = (0.0, 0.0, 0, 0);
    }

    fn close_bar(&mut self) {
        self.bars.push((amplitude_db(self.bar_peak), loudness(self.bar_power / self.bar_frames as f64)));
        (self.bar_power, self.bar_peak, self.bar_frames) = (0.0, 0.0, 0);
    }

    fn spectrum(&mut self, length: usize) {
        let (held, window) = (&self.held, &self.window);
        let shaped = |index: usize| if length == WINDOW { window[index] } else { 0.5 - 0.5 * (2.0 * PI * index as f64 / length as f64).cos() };
        for (index, (re, im)) in self.re.iter_mut().zip(&mut self.im).enumerate() {
            *re = if index < length { held[index] * shaped(index) } else { 0.0 };
            *im = 0.0;
        }
        self.fft.forward(&mut self.re, &mut self.im);
        let power = |(from, to): (usize, usize)| self.re[from..to].iter().zip(&self.im[from..to]).map(|(re, im)| re * re + im * im).sum::<f64>();
        self.whole_power += power(self.whole_bins);
        let bands = self.band_bins.map(power);
        for (sum, band) in self.band_power.iter_mut().zip(bands) {
            *sum += band;
        }
    }
}

pub fn amplitude_db(amplitude: f32) -> f64 {
    if amplitude > 0.0 { (20.0 * f64::from(amplitude).log10()).max(FLOOR_DB) } else { FLOOR_DB }
}

fn power_db(power: f64) -> f64 {
    if power > 0.0 { (10.0 * power.log10()).max(FLOOR_DB) } else { FLOOR_DB }
}

fn loudness(power: f64) -> f64 {
    if power > 0.0 { (LOUDNESS_OFFSET_DB + 10.0 * power.log10()).max(FLOOR_DB) } else { FLOOR_DB }
}

fn integrated(hops: &[f64]) -> Option<f64> {
    if hops.is_empty() {
        return None;
    }
    let mean = |blocks: &[f64]| blocks.iter().sum::<f64>() / blocks.len() as f64;
    let blocks: Vec<f64> = hops.windows(BLOCK_HOPS).step_by(BLOCK_STEP).map(mean).filter(|&power| loudness(power) > ABSOLUTE_GATE_LUFS).collect();
    if blocks.is_empty() {
        return Some(loudness(mean(hops)));
    }
    let gate = loudness(mean(&blocks)) + RELATIVE_GATE_LU;
    let kept: Vec<f64> = blocks.into_iter().filter(|&power| loudness(power) > gate).collect();
    Some(loudness(mean(&kept)))
}

fn weighed(stages: &mut [Biquad; 2], x: f64) -> f64 {
    let shelved = stages[0].tick(x);
    stages[1].tick(shelved)
}

fn weighting(rate: f64) -> [BiquadCoeffs; 2] {
    let k = (PI * SHELF_HZ / rate).tan();
    let lift = 10f64.powf(SHELF_DB / 20.0);
    let spread = lift.powf(SHELF_SPREAD);
    let a0 = 1.0 + k / SHELF_Q + k * k;
    let shelf = BiquadCoeffs { b0: (lift + spread * k / SHELF_Q + k * k) / a0, b1: 2.0 * (k * k - lift) / a0, b2: (lift - spread * k / SHELF_Q + k * k) / a0, a1: 2.0 * (k * k - 1.0) / a0, a2: (1.0 - k / SHELF_Q + k * k) / a0 };
    let k = (PI * RUMBLE_HZ / rate).tan();
    let a0 = 1.0 + k / RUMBLE_Q + k * k;
    let rumble = BiquadCoeffs { b0: 1.0, b1: -2.0, b2: 1.0, a1: 2.0 * (k * k - 1.0) / a0, a2: (1.0 - k / RUMBLE_Q + k * k) / a0 };
    [shelf, rumble]
}

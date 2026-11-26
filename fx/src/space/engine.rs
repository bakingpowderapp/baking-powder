use sdk::dsp::fft::Fft;
use sdk::spectrum::{CURVE, curve_hz};
use std::f64::consts::{PI, TAU};

pub const FRAME: usize = 2048;
pub const HOP: usize = 256;
pub const FILTER: usize = 4096;
pub const FILTER_BINS: usize = FILTER / 2 + 1;
pub const BANDS: usize = 175;
pub const LOOKAHEAD_MAX: usize = 2;
pub const SHIFT: usize = FRAME / 4;
pub const ALIGN: usize = LOOKAHEAD_MAX * HOP + SHIFT;
pub const LATENCY: usize = ALIGN + HOP;
pub const DEPTH_MAX_DB: f64 = 30.0;
const TAPS: usize = FILTER - HOP;
const FADE: usize = 128;
const BINS: usize = FRAME / 2 + 1;
const GRID_STEP: f64 = 0.25;
const SIGNAL_FLOOR_DB: f64 = -48.0;
const SIGNAL_RANGE_DB: f64 = 24.0;
const GATE_UNDER_DB: f64 = 36.0;
const GATE_WIDTH_DB: f64 = 12.0;
const RELEASE_DB: f64 = 6.0206;
const SNAP_DB: f64 = 0.01;
const LOG_FLOOR: f64 = 1e-6;
const POWER_FLOOR: f64 = 1e-20;
const FWHM_TO_SIGMA: f64 = 1.0 / 2.354_820_045;
const LOOKAHEAD_EPSILON: f64 = 1e-9;
const INVERSE_SCALE: f64 = 1.0 / FILTER as f64;
const EMPTY_SUM: f64 = -0.0;

pub fn erb_rate(hz: f64) -> f64 {
    21.4 * (1.0 + 0.00437 * hz).log10()
}

fn hz_of_erb(erb: f64) -> f64 {
    (10f64.powf(erb / 21.4) - 1.0) / 0.00437
}

fn erb_width(hz: f64) -> f64 {
    24.7 * (0.00437 * hz + 1.0)
}

fn bin_hz(bin: usize, n: usize, sample_rate: f64) -> f64 {
    bin as f64 * (1.0 / (n as f64 * (1.0 / sample_rate)))
}

fn smoothstep(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn level_db(power: f64) -> f64 {
    10.0 * power.max(POWER_FLOOR).log10()
}

fn inverse(fft: &Fft, re: &mut [f64], im: &mut [f64], tail: usize) {
    fft.forward_tail(im, re, tail);
}

fn straddle(hz: f64) -> (usize, f64) {
    let position = erb_rate(hz) / GRID_STEP;
    let upper = position.ceil().clamp(1.0, (BANDS - 1) as f64) as usize;
    (upper, (position - (upper - 1) as f64).clamp(0.0, 1.0))
}

fn lerp(cut: &[f64; BANDS], (upper, share): (usize, f64)) -> f64 {
    (1.0 - share) * cut[upper - 1] + share * cut[upper]
}

fn normalize(values: &mut [f64]) {
    for value in values {
        *value *= INVERSE_SCALE;
    }
}

pub struct Grid {
    erb: [f64; BANDS],
    hz: [f64; BANDS],
    member: Vec<Option<(usize, f64)>>,
    between: Vec<(usize, usize, f64)>,
    spread: Vec<(usize, f64)>,
    curve: [(usize, f64); CURVE],
}

impl Grid {
    pub fn new(sample_rate: f64) -> Grid {
        let erb: [f64; BANDS] = std::array::from_fn(|z| z as f64 * GRID_STEP);
        let hz = erb.map(hz_of_erb);
        let freqs: Vec<f64> = (0..BINS).map(|b| bin_hz(b, FRAME, sample_rate)).collect();
        let bins_erb: Vec<f64> = freqs.iter().map(|&f| erb_rate(f)).collect();
        let mut member = vec![None; BINS];
        let mut between = Vec::new();
        for z in 0..BANDS {
            let (lo, hi) = (erb[z] - GRID_STEP / 2.0, erb[z] + GRID_STEP / 2.0);
            let inside: Vec<usize> = (0..BINS).filter(|&b| bins_erb[b] >= lo && bins_erb[b] < hi).collect();
            if inside.is_empty() {
                let upper = freqs.iter().position(|&f| f >= hz[z]).unwrap_or(BINS).clamp(1, BINS - 1);
                let share = (hz[z] - freqs[upper - 1]) / (freqs[upper] - freqs[upper - 1]);
                between.push((z, upper, share));
            } else {
                let weight = 1.0 / inside.len() as f64;
                for b in inside {
                    member[b] = Some((z, weight));
                }
            }
        }
        let spread = (0..FILTER_BINS).map(|b| straddle(bin_hz(b, FILTER, sample_rate))).collect();
        Grid { erb, hz, member, between, spread, curve: std::array::from_fn(|cell| straddle(curve_hz(cell))) }
    }

    fn levels(&self, power: &[f64], out: &mut [f64; BANDS]) {
        out.fill(0.0);
        for (b, member) in self.member.iter().enumerate() {
            if let Some((z, weight)) = member {
                out[*z] += weight * power[b];
            }
        }
        for &(z, upper, share) in &self.between {
            out[z] = (1.0 - share) * power[upper - 1] + share * power[upper];
        }
        for level in out.iter_mut() {
            *level = level_db(*level);
        }
    }

    pub fn range(&self, low_hz: f64, high_hz: f64, out: &mut [f64; BANDS]) {
        let (lo, hi) = (erb_rate(low_hz) - GRID_STEP / 2.0, erb_rate(high_hz) + GRID_STEP / 2.0);
        for (erb, inside) in self.erb.iter().zip(out.iter_mut()) {
            *inside = if *erb >= lo && *erb <= hi { 1.0 } else { 0.0 };
        }
    }

    pub fn kernel(&self, q: f64, kernel: &mut Kernel) {
        for (z, (&hz, &center)) in self.hz.iter().zip(&self.erb).enumerate() {
            let width = erb_width(hz);
            let sigma = (hz / q).max(width) / width * FWHM_TO_SIGMA;
            let row = self.erb.map(|erb| {
                let d = (erb - center) / sigma;
                (-0.5 * d * d).exp()
            });
            let sum = row.iter().fold(0.0, |sum, weight| sum + weight);
            for (column, weight) in kernel.columns.as_chunks_mut::<BANDS>().0.iter_mut().zip(row) {
                column[z] = weight / sum;
            }
        }
    }

    pub fn spread(&self, cut: &[f64; BANDS], out: &mut [f64]) {
        for (bin, &at) in out.iter_mut().zip(&self.spread) {
            *bin = lerp(cut, at).min(DEPTH_MAX_DB);
        }
    }

    pub fn curve(&self, cut: &[f64; BANDS], out: &mut [f32; CURVE]) {
        for (cell, &at) in out.iter_mut().zip(&self.curve) {
            *cell = lerp(cut, at) as f32;
        }
    }
}

pub struct Kernel {
    columns: Vec<f64>,
}

impl Default for Kernel {
    fn default() -> Kernel {
        Kernel { columns: vec![0.0; BANDS * BANDS] }
    }
}

impl Kernel {
    pub fn apply(&self, cut: &[f64; BANDS], out: &mut [f64; BANDS]) {
        out.fill(EMPTY_SUM);
        for (column, c) in self.columns.as_chunks::<BANDS>().0.iter().zip(cut) {
            for (smoothed, weight) in out.iter_mut().zip(column) {
                *smoothed += weight * c;
            }
        }
    }
}

pub struct Analysis {
    fft: Fft,
    window: Vec<f64>,
    scale: f64,
    re: Vec<f64>,
    im: Vec<f64>,
    power: Vec<f64>,
}

impl Default for Analysis {
    fn default() -> Analysis {
        let window: Vec<f64> = (0..FRAME).map(|i| 0.5 - 0.5 * (TAU * i as f64 / FRAME as f64).cos()).collect();
        let sum: f64 = window.iter().sum();
        Analysis { fft: Fft::new(FRAME), window, scale: (2.0 / sum).powi(2), re: vec![0.0; FRAME], im: vec![0.0; FRAME], power: vec![0.0; BINS] }
    }
}

impl Analysis {
    pub fn levels(&mut self, l: &[f32], r: &[f32], grid: &Grid, out: &mut [f64; BANDS]) {
        if l.iter().chain(r).all(|&sample| sample == 0.0) {
            out.fill(level_db(0.0));
            return;
        }
        for (((re, im), (l, r)), w) in self.re.iter_mut().zip(&mut self.im).zip(l.iter().zip(r)).zip(&self.window) {
            *re = f64::from(*l) * w;
            *im = f64::from(*r) * w;
        }
        self.fft.forward(&mut self.re, &mut self.im);
        for (b, power) in self.power.iter_mut().enumerate() {
            let mirror = (FRAME - b) % FRAME;
            let (ar, ai, br, bi) = (self.re[b], self.im[b], self.re[mirror], self.im[mirror]);
            *power = (ar * ar + ai * ai + br * br + bi * bi) * 0.25 * self.scale;
        }
        grid.levels(&self.power, out);
    }
}

pub fn signalled(signal_db: &[f64; BANDS], mask: &[f64; BANDS]) -> bool {
    signal_db.iter().zip(mask).any(|(&signal, &inside)| inside != 0.0 && signal > SIGNAL_FLOOR_DB)
}

pub fn law(signal_db: &[f64; BANDS], main_db: &[f64; BANDS], depth_db: f64, mask: &[f64; BANDS], out: &mut [f64; BANDS]) {
    for (z, cut) in out.iter_mut().enumerate() {
        let strength = ((signal_db[z] - SIGNAL_FLOOR_DB) / SIGNAL_RANGE_DB).clamp(0.0, 1.0);
        let gate = smoothstep((main_db[z] - signal_db[z] + GATE_UNDER_DB) / GATE_WIDTH_DB);
        *cut = depth_db * strength * gate * mask[z];
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Timing {
    pub lookahead: usize,
    pub release_step: f64,
    pub attack: f64,
}

impl Timing {
    pub fn new(attack_ms: f64, recovery_ms: f64, sample_rate: f64) -> Timing {
        let seconds = HOP as f64 / sample_rate;
        let hops = (attack_ms / 1000.0 / seconds - LOOKAHEAD_EPSILON).ceil().max(0.0) as usize;
        Timing { lookahead: hops.min(LOOKAHEAD_MAX), release_step: RELEASE_DB * seconds / (recovery_ms / 1000.0), attack: (-seconds / (attack_ms / 1000.0)).exp() }
    }
}

pub struct Lookahead {
    cuts: [[f64; BANDS]; LOOKAHEAD_MAX + 1],
    newest: usize,
}

impl Default for Lookahead {
    fn default() -> Lookahead {
        Lookahead { cuts: [[0.0; BANDS]; LOOKAHEAD_MAX + 1], newest: 0 }
    }
}

impl Lookahead {
    pub fn push(&mut self, cut: &[f64; BANDS]) {
        self.newest = (self.newest + 1) % (LOOKAHEAD_MAX + 1);
        self.cuts[self.newest] = *cut;
    }

    pub fn ahead(&self, lookahead: usize, out: &mut [f64; BANDS]) {
        let oldest = (self.newest + 1) % (LOOKAHEAD_MAX + 1);
        *out = self.cuts[oldest];
        for step in 1..=lookahead.min(LOOKAHEAD_MAX) {
            for (ahead, cut) in out.iter_mut().zip(&self.cuts[(oldest + step) % (LOOKAHEAD_MAX + 1)]) {
                *ahead = ahead.max(*cut);
            }
        }
    }

    pub fn clear(&mut self) {
        *self = Lookahead::default();
    }
}

pub struct Envelope {
    release: [f64; BANDS],
    attack: [f64; BANDS],
}

impl Default for Envelope {
    fn default() -> Envelope {
        Envelope { release: [0.0; BANDS], attack: [0.0; BANDS] }
    }
}

impl Envelope {
    pub fn step(&mut self, ahead: &[f64; BANDS], timing: &Timing, out: &mut [f64; BANDS]) -> f64 {
        let mut deepest = 0.0f64;
        for (z, cut) in out.iter_mut().enumerate() {
            self.release[z] = ahead[z].max(self.release[z] - timing.release_step);
            self.attack[z] = timing.attack * self.attack[z] + (1.0 - timing.attack) * self.release[z];
            *cut = if self.attack[z] < SNAP_DB { 0.0 } else { self.attack[z] };
            deepest = deepest.max(*cut);
        }
        deepest
    }

    pub fn clear(&mut self) {
        *self = Envelope::default();
    }
}

pub struct Response {
    re: Vec<f64>,
    im: Vec<f64>,
}

impl Default for Response {
    fn default() -> Response {
        Response { re: vec![0.0; FILTER], im: vec![0.0; FILTER] }
    }
}

pub struct Designer {
    fft: Fft,
    re: Vec<f64>,
    im: Vec<f64>,
    fade: Vec<f64>,
}

impl Default for Designer {
    fn default() -> Designer {
        Designer { fft: Fft::new(FILTER), re: vec![0.0; FILTER], im: vec![0.0; FILTER], fade: (0..FADE).map(|i| 0.5 + 0.5 * (PI * i as f64 / FADE as f64).cos()).collect() }
    }
}

impl Designer {
    pub fn design(&mut self, cut_bins: &[f64], out: &mut Response) {
        for (b, cut) in cut_bins.iter().enumerate().take(FILTER_BINS) {
            let log_gain = 10f64.powf(-cut / 20.0).max(LOG_FLOOR).ln();
            self.re[b] = log_gain;
            if b > 0 && b < FILTER / 2 {
                self.re[FILTER - b] = log_gain;
            }
        }
        self.im.fill(0.0);
        inverse(&self.fft, &mut self.re, &mut self.im, FILTER);
        normalize(&mut self.re[..=FILTER / 2]);
        for cepstrum in &mut self.re[1..FILTER / 2] {
            *cepstrum *= 2.0;
        }
        self.re[FILTER / 2 + 1..].fill(0.0);
        self.im.fill(0.0);
        self.fft.forward(&mut self.re, &mut self.im);
        for (re, im) in self.re.iter_mut().zip(&mut self.im) {
            let (sin, cos) = im.sin_cos();
            let magnitude = re.exp();
            *re = magnitude * cos;
            *im = magnitude * sin;
        }
        inverse(&self.fft, &mut self.re, &mut self.im, FILTER);
        normalize(&mut self.re[..TAPS]);
        for (tap, fade) in self.re[TAPS - FADE..TAPS].iter_mut().zip(&self.fade) {
            *tap *= fade;
        }
        self.re[TAPS..].fill(0.0);
        self.im.fill(0.0);
        self.fft.forward(&mut self.re, &mut self.im);
        std::mem::swap(&mut self.re, &mut out.re);
        std::mem::swap(&mut self.im, &mut out.im);
    }
}

pub struct Convolver {
    fft: Fft,
    re: Vec<f64>,
    im: Vec<f64>,
    zr: Vec<f64>,
    zi: Vec<f64>,
    fade_in: Vec<f64>,
    now_l: Vec<f64>,
    now_r: Vec<f64>,
    before_l: Vec<f64>,
    before_r: Vec<f64>,
}

impl Default for Convolver {
    fn default() -> Convolver {
        Convolver {
            fft: Fft::new(FILTER),
            re: vec![0.0; FILTER],
            im: vec![0.0; FILTER],
            zr: vec![0.0; FILTER],
            zi: vec![0.0; FILTER],
            fade_in: (0..HOP).map(|i| 0.5 - 0.5 * (PI * (i as f64 + 0.5) / HOP as f64).cos()).collect(),
            now_l: vec![0.0; HOP],
            now_r: vec![0.0; HOP],
            before_l: vec![0.0; HOP],
            before_r: vec![0.0; HOP],
        }
    }
}

impl Convolver {
    pub fn block(&mut self, l: &[f32], r: &[f32], now: Option<&Response>, before: Option<&Response>, out_l: &mut [f64], out_r: &mut [f64]) {
        for ((re, im), (l, r)) in self.re.iter_mut().zip(&mut self.im).zip(l.iter().zip(r)) {
            *re = f64::from(*l);
            *im = f64::from(*r);
        }
        self.fft.forward(&mut self.re, &mut self.im);
        let (dry_l, dry_r) = (&l[TAPS..], &r[TAPS..]);
        match now {
            Some(response) => {
                self.filtered(response);
                self.now_l.copy_from_slice(&self.zr[TAPS..]);
                self.now_r.copy_from_slice(&self.zi[TAPS..]);
            }
            None => {
                for ((now_l, now_r), (l, r)) in self.now_l.iter_mut().zip(&mut self.now_r).zip(dry_l.iter().zip(dry_r)) {
                    *now_l = f64::from(*l);
                    *now_r = f64::from(*r);
                }
            }
        }
        match before {
            Some(response) => {
                self.filtered(response);
                self.before_l.copy_from_slice(&self.zr[TAPS..]);
                self.before_r.copy_from_slice(&self.zi[TAPS..]);
            }
            None => {
                for ((before_l, before_r), (l, r)) in self.before_l.iter_mut().zip(&mut self.before_r).zip(dry_l.iter().zip(dry_r)) {
                    *before_l = f64::from(*l);
                    *before_r = f64::from(*r);
                }
            }
        }
        for (i, ramp) in self.fade_in.iter().enumerate() {
            out_l[i] = ramp * self.now_l[i] + (1.0 - ramp) * self.before_l[i];
            out_r[i] = ramp * self.now_r[i] + (1.0 - ramp) * self.before_r[i];
        }
    }

    fn filtered(&mut self, response: &Response) {
        let spectra = self.re.iter().zip(&self.im).zip(response.re.iter().zip(&response.im));
        for ((zr, zi), ((xr, xi), (hr, hi))) in self.zr.iter_mut().zip(&mut self.zi).zip(spectra) {
            *zr = xr * hr - xi * hi;
            *zi = xr * hi + xi * hr;
        }
        inverse(&self.fft, &mut self.zr, &mut self.zi, HOP);
        normalize(&mut self.zr[TAPS..]);
        normalize(&mut self.zi[TAPS..]);
    }
}

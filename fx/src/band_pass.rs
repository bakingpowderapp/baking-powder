use sdk::dsp::svf::Svf;
use sdk::dsp::{self, Smooth};
use crate::params::{self, PASS_BANDS, PassBand};
use std::f64::consts::PI;

const GLIDE_SECONDS: f64 = 0.005;
const NYQUIST_SHARE: f64 = 0.45;
const SILENT: f64 = 1e-6;
const TICKLE: f64 = 1e-25;

struct Band {
    pitch: Smooth,
    width: Smooth,
    level: Smooth,
    l: Svf,
    r: Svf,
}

impl Band {
    fn new(sample_rate: f64) -> Band {
        let smooth = || Smooth::new(0.0, GLIDE_SECONDS, sample_rate);
        Band { pitch: smooth(), width: smooth(), level: smooth(), l: Svf::default(), r: Svf::default() }
    }

    fn snap(&mut self, band: &PassBand) {
        self.pitch.snap(band.hz.log2());
        self.width.snap(band.q.ln());
        self.level.snap(band.level);
    }

    fn wake(&mut self, band: &PassBand) {
        self.snap(band);
        self.l.reset();
        self.r.reset();
    }

    fn tick(&mut self, l: f64, r: f64, band: &PassBand, top: f64, sample_rate: f64) -> (f64, f64) {
        let level = self.level.tick(band.level);
        if band.level <= 0.0 && level < SILENT {
            self.wake(band);
            return (0.0, 0.0);
        }
        let hz = self.pitch.tick(band.hz.log2()).exp2().min(top);
        let k = 1.0 / self.width.tick(band.q.ln()).exp();
        let g = (PI * hz / sample_rate).tan();
        (level * self.l.tick(l, g, k).bp, level * self.r.tick(r, g, k).bp)
    }
}

pub struct Passed {
    pub l: f64,
    pub r: f64,
    pub gain: f64,
}

pub struct BandPass {
    sample_rate: f64,
    bands: [Band; PASS_BANDS],
    trim: Smooth,
    tickle: f64,
    awake: bool,
}

impl BandPass {
    pub fn new(sample_rate: f64) -> BandPass {
        BandPass { sample_rate, bands: std::array::from_fn(|_| Band::new(sample_rate)), trim: Smooth::new(1.0, GLIDE_SECONDS, sample_rate), tickle: TICKLE, awake: false }
    }

    pub fn rest(&mut self) {
        self.awake = false;
    }

    pub fn begin(&mut self, p: &params::BandPass) -> f64 {
        let trim = dsp::db_to_gain(p.trim_db);
        if !self.awake {
            for (band, target) in self.bands.iter_mut().zip(&p.bands) {
                band.wake(target);
            }
            self.trim.snap(trim);
            self.awake = true;
        }
        self.tickle = -self.tickle;
        trim
    }

    pub fn tick(&mut self, l: f64, r: f64, p: &params::BandPass, trim: f64) -> Passed {
        let (sample_rate, top) = (self.sample_rate, NYQUIST_SHARE * self.sample_rate);
        let (mut out_l, mut out_r) = (0.0, 0.0);
        for (band, target) in self.bands.iter_mut().zip(&p.bands) {
            let (band_l, band_r) = band.tick(l + self.tickle, r + self.tickle, target, top, sample_rate);
            out_l += band_l;
            out_r += band_r;
        }
        Passed { l: out_l, r: out_r, gain: self.trim.tick(trim) }
    }
}

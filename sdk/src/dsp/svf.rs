use crate::dsp::Halfband;
use std::f64::consts::{FRAC_1_SQRT_2, PI};

pub const DRIVE_ENGAGE: f64 = 1e-4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tap {
    Low,
    Band,
    High,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Svf {
    pub s1: f64,
    pub s2: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct SvfOut {
    pub lp: f64,
    pub bp: f64,
    pub hp: f64,
}

impl SvfOut {
    pub fn at(self, tap: Tap) -> f64 {
        match tap {
            Tap::Low => self.lp,
            Tap::Band => self.bp,
            Tap::High => self.hp,
        }
    }
}

fn bent(v: f64, headroom: f64) -> f64 {
    headroom * (v / headroom).tanh()
}

impl Svf {
    pub fn tick(&mut self, input: f64, g: f64, k: f64) -> SvfOut {
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let v3 = input - self.s2;
        let v1 = a1 * self.s1 + a2 * v3;
        let v2 = self.s2 + a2 * self.s1 + a3 * v3;
        self.s1 = 2.0 * v1 - self.s1;
        self.s2 = 2.0 * v2 - self.s2;
        SvfOut { lp: v2, bp: v1, hp: input - k * v1 - v2 }
    }

    pub fn tick_driven(&mut self, input: f64, g: f64, k: f64, drive: f64) -> SvfOut {
        let headroom = 1.0 / drive;
        let x = bent((1.0 + 4.0 * drive) * input, headroom);
        let hp = (x - (g + k) * self.s1 - self.s2) / (1.0 + g * (g + k));
        let v1 = g * bent(hp, headroom);
        let bp = v1 + self.s1;
        self.s1 = bp + v1;
        let v2 = g * bent(bp, headroom);
        let lp = v2 + self.s2;
        self.s2 = lp + v2;
        SvfOut { lp, bp, hp }
    }

    pub fn reset(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Shaped {
    pub svf: Svf,
    up: Halfband,
    down: Halfband,
}

impl Shaped {
    pub fn tick2x(&mut self, x: f64, g: f64, k: f64, drive: f64, tap: Tap) -> f64 {
        let (first, second) = self.up.upsample(x as f32);
        let mut step = |x: f32| {
            let x = f64::from(x);
            let out = if drive > DRIVE_ENGAGE { self.svf.tick_driven(x, g, k, drive) } else { self.svf.tick(x, g, k) };
            out.at(tap) as f32
        };
        let (a, b) = (step(first), step(second));
        let y = f64::from(self.down.downsample(a, b));
        if y.is_finite() {
            y
        } else {
            self.reset();
            0.0
        }
    }

    pub fn reset(&mut self) {
        self.svf.reset();
        self.up.reset();
        self.down.reset();
    }
}

pub fn svf_damping(resonance: f64) -> f64 {
    let r = resonance.clamp(0.0, 1.0);
    let q_min: f64 = FRAC_1_SQRT_2;
    let q_max: f64 = 25.0;
    let q = q_min * (q_max / q_min).powf(r.powf(1.7));
    1.0 / q
}

#[derive(Clone, Copy, Debug)]
pub struct BiquadCoeffs {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

impl Default for BiquadCoeffs {
    fn default() -> BiquadCoeffs {
        BiquadCoeffs { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Biquad {
    pub coeffs: BiquadCoeffs,
    x1: f64,
    x2: f64,
    pub y1: f64,
    y2: f64,
}

impl Biquad {
    pub fn with(coeffs: BiquadCoeffs) -> Biquad {
        Biquad { coeffs, ..Biquad::default() }
    }

    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }

    pub fn tick(&mut self, x: f64) -> f64 {
        self.run(self.coeffs, x)
    }

    pub fn settle(&mut self, c: BiquadCoeffs, x: f64) -> f64 {
        let y = x * (c.b0 + c.b1 + c.b2) / (1.0 + c.a1 + c.a2);
        (self.x1, self.x2, self.y1, self.y2) = (x, x, y, y);
        y
    }

    pub fn run(&mut self, c: BiquadCoeffs, x: f64) -> f64 {
        let y = c.b0 * x + c.b1 * self.x1 + c.b2 * self.x2 - c.a1 * self.y1 - c.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct StereoBiquad {
    x1: [f64; 2],
    x2: [f64; 2],
    y1: [f64; 2],
    y2: [f64; 2],
}

impl StereoBiquad {
    pub fn reset(&mut self) {
        *self = StereoBiquad::default();
    }

    #[inline]
    pub fn run(&mut self, c: BiquadCoeffs, x: [f64; 2]) -> [f64; 2] {
        let y = [0, 1].map(|i| c.b0 * x[i] + c.b1 * self.x1[i] + c.b2 * self.x2[i] - c.a1 * self.y1[i] - c.a2 * self.y2[i]);
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

fn shelf(gain_db: f64, f0: f64, sample_rate: f64, high: bool) -> BiquadCoeffs {
    let w0 = 2.0 * PI * f0 / sample_rate;
    shelved(gain_db, w0, (w0.sin() * 0.5) * 2f64.sqrt(), high)
}

fn shelf_q(gain_db: f64, f0: f64, q: f64, sample_rate: f64, high: bool) -> BiquadCoeffs {
    let w0 = 2.0 * PI * f0 / sample_rate;
    shelved(gain_db, w0, w0.sin() / (2.0 * q), high)
}

fn shelved(gain_db: f64, w0: f64, alpha: f64, high: bool) -> BiquadCoeffs {
    let a = 10f64.powf(gain_db / 40.0);
    let cosw = if high { -w0.cos() } else { w0.cos() };
    let two_sq_aa = 2.0 * a.sqrt() * alpha;
    let sign = if high { -1.0 } else { 1.0 };
    let b0 = a * ((a + 1.0) - (a - 1.0) * cosw + two_sq_aa);
    let b1 = sign * 2.0 * a * ((a - 1.0) - (a + 1.0) * cosw);
    let b2 = a * ((a + 1.0) - (a - 1.0) * cosw - two_sq_aa);
    let a0 = (a + 1.0) + (a - 1.0) * cosw + two_sq_aa;
    let a1 = sign * -2.0 * ((a - 1.0) + (a + 1.0) * cosw);
    let a2 = (a + 1.0) + (a - 1.0) * cosw - two_sq_aa;
    BiquadCoeffs { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 }
}

pub fn low_shelf(gain_db: f64, f0: f64, sample_rate: f64) -> BiquadCoeffs {
    shelf(gain_db, f0, sample_rate, false)
}

pub fn high_shelf(gain_db: f64, f0: f64, sample_rate: f64) -> BiquadCoeffs {
    shelf(gain_db, f0, sample_rate, true)
}

pub fn low_shelf_q(gain_db: f64, f0: f64, q: f64, sample_rate: f64) -> BiquadCoeffs {
    shelf_q(gain_db, f0, q, sample_rate, false)
}

pub fn high_shelf_q(gain_db: f64, f0: f64, q: f64, sample_rate: f64) -> BiquadCoeffs {
    shelf_q(gain_db, f0, q, sample_rate, true)
}

fn rbj(hz: f64, q: f64, sample_rate: f64) -> (f64, f64, f64) {
    let w0 = 2.0 * PI * hz / sample_rate;
    (w0.cos(), w0.sin(), w0.sin() / (2.0 * q))
}

fn normalized(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> BiquadCoeffs {
    BiquadCoeffs { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 }
}

pub fn low_pass(hz: f64, q: f64, sample_rate: f64) -> BiquadCoeffs {
    let (cosw, _, alpha) = rbj(hz, q, sample_rate);
    normalized((1.0 - cosw) / 2.0, 1.0 - cosw, (1.0 - cosw) / 2.0, 1.0 + alpha, -2.0 * cosw, 1.0 - alpha)
}

pub fn high_pass(hz: f64, q: f64, sample_rate: f64) -> BiquadCoeffs {
    let (cosw, _, alpha) = rbj(hz, q, sample_rate);
    normalized((1.0 + cosw) / 2.0, -(1.0 + cosw), (1.0 + cosw) / 2.0, 1.0 + alpha, -2.0 * cosw, 1.0 - alpha)
}

pub fn high_pass_6db(hz: f64, sample_rate: f64) -> BiquadCoeffs {
    let k = (PI * hz / sample_rate).tan();
    normalized(1.0, -1.0, 0.0, 1.0 + k, k - 1.0, 0.0)
}

pub fn low_pass_6db(hz: f64, sample_rate: f64) -> BiquadCoeffs {
    let k = (PI * hz / sample_rate).tan();
    normalized(k, k, 0.0, 1.0 + k, k - 1.0, 0.0)
}

pub fn band_pass(hz: f64, q: f64, sample_rate: f64) -> BiquadCoeffs {
    let (cosw, _, alpha) = rbj(hz, q, sample_rate);
    normalized(alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cosw, 1.0 - alpha)
}

pub fn notch(hz: f64, q: f64, sample_rate: f64) -> BiquadCoeffs {
    let (cosw, _, alpha) = rbj(hz, q, sample_rate);
    normalized(1.0, -2.0 * cosw, 1.0, 1.0 + alpha, -2.0 * cosw, 1.0 - alpha)
}

pub fn bell(hz: f64, q: f64, gain_db: f64, sample_rate: f64) -> BiquadCoeffs {
    let a = 10f64.powf(gain_db / 40.0);
    let (cosw, _, alpha) = rbj(hz, q, sample_rate);
    normalized(1.0 + alpha * a, -2.0 * cosw, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cosw, 1.0 - alpha / a)
}

impl BiquadCoeffs {
    pub fn toward(&mut self, target: BiquadCoeffs, a: f64) {
        self.b0 += a * (target.b0 - self.b0);
        self.b1 += a * (target.b1 - self.b1);
        self.b2 += a * (target.b2 - self.b2);
        self.a1 += a * (target.a1 - self.a1);
        self.a2 += a * (target.a2 - self.a2);
    }

    pub fn magnitude_db(&self, hz: f64, sample_rate: f64) -> f64 {
        let w = 2.0 * PI * hz / sample_rate;
        let (c1, c2) = (w.cos(), (2.0 * w).cos());
        let numerator = self.b0 * self.b0 + self.b1 * self.b1 + self.b2 * self.b2 + 2.0 * (self.b0 * self.b1 + self.b1 * self.b2) * c1 + 2.0 * self.b0 * self.b2 * c2;
        let denominator = 1.0 + self.a1 * self.a1 + self.a2 * self.a2 + 2.0 * (self.a1 + self.a1 * self.a2) * c1 + 2.0 * self.a2 * c2;
        10.0 * (numerator / denominator.max(1e-24)).max(1e-24).log10()
    }
}

use crate::dsp::svf::{Biquad, BiquadCoeffs, band_pass, high_pass, low_pass};
use crate::dsp::{OnePoleHp, lp_coeff, one_pole_lp, settle};
use std::f64::consts::{FRAC_1_SQRT_2, TAU};

const NYQUIST_GUARD: f64 = 0.45;
const POLES: usize = 4;
const TWOS: usize = 2;
const FLOOR: f64 = 1e-24;

crate::choice!(pub Pass { Low = "LP" / "low-pass", High = "HP" / "high-pass", Band = "BP" / "band-pass" });

crate::choice!(pub Slope { Six = "6" / "6", Twelve = "12" / "12", TwentyFour = "24" / "24" });

crate::params!(Filter {
    flag on = false, "on";
    choice pass: Pass = Pass::Low, "type";
    choice slope: Slope = Slope::Twelve, "slopeDb";
    number hz = 1000.0, "cutoffHz", Filter::MIN_HZ..=Filter::MAX_HZ;
    flag resonant = false, "resonant";
    number q = Filter::FLAT_Q, "q", Filter::MIN_Q..=Filter::MAX_Q;
});

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Path {
    Off,
    Poles(Pass, usize),
    Twos(usize),
}

impl Filter {
    pub const MIN_HZ: f64 = 20.0;
    pub const MAX_HZ: f64 = 20_000.0;
    pub const FLAT_Q: f64 = FRAC_1_SQRT_2;
    pub const MIN_Q: f64 = 0.25;
    pub const MAX_Q: f64 = 10.0;
    pub const LOW_CUT: Filter = Filter { on: true, pass: Pass::High, slope: Slope::Six, hz: 80.0, ..Filter::DEFAULT };
    pub const HIGH_CUT: Filter = Filter { on: true, slope: Slope::Six, hz: 16_000.0, ..Filter::DEFAULT };

    pub fn peaked(&self) -> bool {
        self.pass == Pass::Band || (self.resonant && self.slope != Slope::Six)
    }

    pub fn rings(&self) -> bool {
        self.on && self.peaked() && self.pass != Pass::Band
    }

    pub fn path(&self) -> Path {
        match (self.on, self.peaked(), self.slope) {
            (false, _, _) => Path::Off,
            (true, true, Slope::TwentyFour) => Path::Twos(2),
            (true, true, _) => Path::Twos(1),
            (true, false, Slope::Six) => Path::Poles(self.pass, 1),
            (true, false, Slope::Twelve) => Path::Poles(self.pass, 2),
            (true, false, Slope::TwentyFour) => Path::Poles(self.pass, 4),
        }
    }

    pub fn magnitude_db(&self, hz: f64, sample_rate: f64) -> f64 {
        let coeffs = Coeffs::of(self, sample_rate);
        let cos = (TAU * hz / sample_rate).cos();
        let pole = f64::from(coeffs.pole);
        let under = 1.0 - 2.0 * pole * cos + pole * pole;
        match self.path() {
            Path::Off => 0.0,
            Path::Poles(Pass::High, count) => count as f64 * 10.0 * (pole * pole * (2.0 - 2.0 * cos) / under).max(FLOOR).log10(),
            Path::Poles(_, count) => count as f64 * 10.0 * ((1.0 - pole) * (1.0 - pole) / under).max(FLOOR).log10(),
            Path::Twos(count) => count as f64 * coeffs.two.magnitude_db(hz, sample_rate),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Coeffs {
    pole: f32,
    two: BiquadCoeffs,
}

impl Coeffs {
    pub fn of(filter: &Filter, sample_rate: f64) -> Coeffs {
        let hz = filter.hz.min(sample_rate * NYQUIST_GUARD);
        let two = match filter.pass {
            Pass::Low => low_pass(hz, filter.q, sample_rate),
            Pass::High => high_pass(hz, filter.q, sample_rate),
            Pass::Band => band_pass(hz, filter.q, sample_rate),
        };
        Coeffs { pole: lp_coeff(filter.hz, sample_rate), two }
    }

    pub fn approach(&mut self, target: &Coeffs, a: f32) {
        self.pole = settle(self.pole, target.pole, a);
        self.two.toward(target.two, f64::from(a));
    }
}

impl PartialEq for Coeffs {
    fn eq(&self, other: &Coeffs) -> bool {
        let (mine, theirs) = (self.two, other.two);
        self.pole == other.pole && [mine.b0, mine.b1, mine.b2, mine.a1, mine.a2] == [theirs.b0, theirs.b1, theirs.b2, theirs.a1, theirs.a2]
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Tuned {
    from: Filter,
    pub path: Path,
    pub coeffs: Coeffs,
}

impl Tuned {
    pub fn of(filter: &Filter, sample_rate: f64) -> Tuned {
        Tuned { from: *filter, path: filter.path(), coeffs: Coeffs::of(filter, sample_rate) }
    }

    pub fn tune(&mut self, filter: &Filter, sample_rate: f64) {
        if *filter != self.from {
            *self = Tuned::of(filter, sample_rate);
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct State {
    path: Path,
    high: [OnePoleHp; POLES],
    low: [f32; POLES],
    two: [Biquad; TWOS],
}

impl Default for State {
    fn default() -> State {
        State { path: Path::Off, high: [OnePoleHp::default(); POLES], low: [0.0; POLES], two: [Biquad::default(); TWOS] }
    }
}

impl State {
    pub fn tick(&mut self, x: f32, path: Path, coeffs: &Coeffs) -> f32 {
        if path != self.path {
            *self = State { path, ..State::default() };
        }
        let y = match path {
            Path::Off => return x,
            Path::Poles(Pass::High, count) => self.high[..count].iter_mut().fold(x, |y, stage| stage.tick(y, coeffs.pole)),
            Path::Poles(_, count) => self.low[..count].iter_mut().fold(x, |y, held| {
                *held = one_pole_lp(y, coeffs.pole, *held);
                *held
            }),
            Path::Twos(count) => self.two[..count].iter_mut().fold(f64::from(x), |y, stage| stage.run(coeffs.two, y)) as f32,
        };
        if y.is_finite() {
            return y;
        }
        *self = State { path, ..State::default() };
        0.0
    }
}

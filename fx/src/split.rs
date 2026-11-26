use sdk::dsp::crossover::{FirSplit, Line};
use sdk::dsp::svf::{BiquadCoeffs, StereoBiquad, high_pass, high_pass_6db, low_pass, low_pass_6db};
use sdk::dsp::smoothing;
use crate::params::{FreakSlope, MOTION_BANDS};
use std::f64::consts::{FRAC_1_SQRT_2, PI};

pub(crate) const BANDS: usize = MOTION_BANDS;
const SPLITS: usize = BANDS - 1;
const SPLIT_STAGES: usize = 3;
const PASS_STAGES: usize = 2;
const BUTTERWORTH_ORDER: f64 = 5.0;
const TWO_POLE_Q: f64 = 0.5;
const SMOOTH_SECONDS: f64 = 0.030;
pub(crate) const DENORMAL: f32 = 1e-20;
const TOP_SHARE: f64 = 0.45;
const FLOOR_HZ: f64 = 20.0;
pub(crate) const HOP: usize = 64;

pub(crate) fn spaced<const N: usize>(crossovers_hz: [f64; N], ratio: f64, count: usize, sample_rate: f64) -> [f64; N] {
    let mut hz = crossovers_hz;
    for s in 1..N {
        hz[s] = hz[s].max(hz[s - 1] * ratio);
    }
    for hz in &mut hz {
        *hz = hz.clamp(FLOOR_HZ, TOP_SHARE * sample_rate);
    }
    for s in (1..count.saturating_sub(1)).rev() {
        hz[s - 1] = hz[s - 1].min(hz[s] / ratio);
    }
    hz
}

pub(crate) fn butterworth_q(k: f64) -> f64 {
    0.5 / (k * PI / BUTTERWORTH_ORDER).cos()
}

pub(crate) fn cascade(stages: &mut [StereoBiquad], coeffs: &[BiquadCoeffs], x: [f64; 2]) -> [f64; 2] {
    stages.iter_mut().zip(coeffs).fold(x, |y, (stage, c)| stage.run(*c, y))
}

pub(crate) fn coefficient_bits(c: &BiquadCoeffs) -> [u64; 5] {
    let BiquadCoeffs { b0, b1, b2, a1, a2 } = *c;
    [b0, b1, b2, a1, a2].map(f64::to_bits)
}

fn allpass_of(c: BiquadCoeffs) -> BiquadCoeffs {
    if c.a2 == 0.0 { BiquadCoeffs { b0: c.a1, b1: 1.0, b2: 0.0, a1: c.a1, a2: 0.0 } } else { BiquadCoeffs { b0: c.a2, b1: c.a1, b2: 1.0, a1: c.a1, a2: c.a2 } }
}

fn allpass_half(c: BiquadCoeffs) -> BiquadCoeffs {
    BiquadCoeffs { b0: 0.5 * c.a1, b1: 1.0, b2: 0.0, a1: 0.5 * c.a1, a2: 0.0 }
}

pub(crate) struct Crossover {
    low: [[StereoBiquad; SPLIT_STAGES]; SPLITS],
    high: [[StereoBiquad; SPLIT_STAGES]; SPLITS],
    allpass: [[[StereoBiquad; PASS_STAGES]; SPLITS - 1]; SPLITS - 1],
    line: [Line; 2],
}

impl Crossover {
    pub(crate) fn new(fir: &FirSplit) -> Crossover {
        Crossover { low: Default::default(), high: Default::default(), allpass: Default::default(), line: std::array::from_fn(|_| Line::new(fir.taps())) }
    }

    fn split(&mut self, x: [f32; 2], tickle: f32, count: usize, f: &Filters, fir: &FirSplit) -> [[f32; 2]; BANDS] {
        let fed = [x[0] + tickle, x[1] + tickle];
        self.divide(fed, fed.map(f64::from), count, f, fir).0.map(|band| band.map(|y| y as f32))
    }

    pub(crate) fn divide(&mut self, fed: [f32; 2], mut rest: [f64; 2], count: usize, f: &Filters, fir: &FirSplit) -> ([[f64; 2]; BANDS], Option<[f32; 2]>) {
        let [line_l, line_r] = &mut self.line;
        let windows = [line_l.push(fed[0]), line_r.push(fed[1])];
        let mut bands = [[0.0f64; 2]; BANDS];
        for (s, band) in bands.iter_mut().enumerate().take(count - 1) {
            *band = cascade(&mut self.low[s], &f.low[s], rest);
            rest = cascade(&mut self.high[s], &f.high[s], rest).map(|y| f.sign * y);
        }
        bands[count - 1] = rest;
        for s in 1..count - 1 {
            for (band, stages) in bands.iter_mut().zip(self.allpass[s - 1].iter_mut()).take(s) {
                *band = cascade(stages, &f.pass[s], *band);
            }
        }
        if f.linear {
            let mut split = [[0.0f32; BANDS]; 2];
            let [split_l, split_r] = &mut split;
            fir.split_each(windows, [&mut split_l[..count], &mut split_r[..count]]);
            return (std::array::from_fn(|b| [f64::from(split[0][b]), f64::from(split[1][b])]), Some([windows[0][fir.half()], windows[1][fir.half()]]));
        }
        (bands, None)
    }

    fn warm(&mut self, x: [f32; 2]) {
        let [line_l, line_r] = &mut self.line;
        line_l.push(x[0]);
        line_r.push(x[1]);
    }

    pub(crate) fn idle(&mut self, count: usize) {
        let splits = self.low.iter_mut().skip(count - 1).chain(self.high.iter_mut().skip(count - 1)).flatten();
        let passes = self.allpass.iter_mut().skip(count.saturating_sub(2)).flatten().flatten();
        for stage in splits.chain(passes) {
            stage.reset();
        }
    }

    pub(crate) fn rest(&mut self) {
        self.idle(1);
        for line in &mut self.line {
            line.clear();
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Filters {
    low: [[BiquadCoeffs; SPLIT_STAGES]; SPLITS],
    high: [[BiquadCoeffs; SPLIT_STAGES]; SPLITS],
    pass: [[BiquadCoeffs; PASS_STAGES]; SPLITS],
    sign: f64,
    pub(crate) hz: [f64; SPLITS],
    pub(crate) linear: bool,
}

impl Filters {
    pub(crate) fn target(voicing: Voicing, sr: f64) -> Filters {
        let hz = spaced(voicing.crossovers.map(f64::from_bits), f64::from_bits(voicing.ratio), voicing.count, sr);
        let unity = BiquadCoeffs::default();
        let (low, high, sign) = match voicing.slope {
            FreakSlope::Linkwitz | FreakSlope::Linear => {
                let low = hz.map(|hz| [low_pass(hz, FRAC_1_SQRT_2, sr), low_pass(hz, FRAC_1_SQRT_2, sr), unity]);
                let high = hz.map(|hz| [high_pass(hz, FRAC_1_SQRT_2, sr), high_pass(hz, FRAC_1_SQRT_2, sr), unity]);
                (low, high, 1.0)
            }
            FreakSlope::Butterworth => {
                let low = hz.map(|hz| [low_pass_6db(hz, sr), low_pass(hz, butterworth_q(1.0), sr), low_pass(hz, butterworth_q(2.0), sr)]);
                let high = hz.map(|hz| [high_pass_6db(hz, sr), high_pass(hz, butterworth_q(1.0), sr), high_pass(hz, butterworth_q(2.0), sr)]);
                (low, high, -1.0)
            }
            FreakSlope::TwoPole => {
                let low = hz.map(|hz| [low_pass(hz, TWO_POLE_Q, sr), unity, unity]);
                let high = hz.map(|hz| [high_pass(hz, TWO_POLE_Q, sr), unity, unity]);
                (low, high, -1.0)
            }
        };
        let pass = low.map(|stages| match voicing.slope {
            FreakSlope::Linkwitz | FreakSlope::Linear => [allpass_of(stages[0]), unity],
            FreakSlope::Butterworth => [allpass_of(stages[0]), allpass_of(stages[2])],
            FreakSlope::TwoPole => [allpass_half(stages[0]), unity],
        });
        Filters { low, high, pass, sign, hz, linear: voicing.slope == FreakSlope::Linear && voicing.count > 1 }
    }

    pub(crate) fn toward(&mut self, target: &Filters, a: f64) {
        self.sign = target.sign;
        self.linear = target.linear;
        for (hz, t) in self.hz.iter_mut().zip(target.hz) {
            *hz += a * (t - *hz);
        }
        let splits = self.low.iter_mut().flatten().zip(target.low.iter().flatten()).chain(self.high.iter_mut().flatten().zip(target.high.iter().flatten())).chain(self.pass.iter_mut().flatten().zip(target.pass.iter().flatten()));
        for (c, t) in splits {
            c.toward(*t, a);
        }
    }

    fn still(&self, target: &Filters, a: f64) -> bool {
        let mut next = *self;
        next.toward(target, a);
        next.bits().eq(self.bits())
    }

    pub(crate) fn bits(&self) -> impl Iterator<Item = u64> {
        let Filters { low, high, pass, sign, hz, linear } = self;
        let coefficients = low.iter().chain(high).flatten().chain(pass.iter().flatten());
        coefficients.flat_map(coefficient_bits).chain(hz.iter().chain([sign]).map(|x| x.to_bits())).chain([u64::from(*linear)])
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Voicing {
    crossovers: [u64; SPLITS],
    slope: FreakSlope,
    count: usize,
    ratio: u64,
}

impl Voicing {
    pub(crate) fn new(crossovers_hz: [f64; SPLITS], slope: FreakSlope, count: usize, ratio: f64) -> Voicing {
        Voicing { crossovers: crossovers_hz.map(f64::to_bits), slope, count, ratio: ratio.to_bits() }
    }
}


pub(crate) struct Split {
    sample_rate: f64,
    smooth: f32,
    designed: Option<(Voicing, Filters)>,
    filters: Option<Filters>,
    filters_settled_for: Option<Voicing>,
    count: usize,
    crossover: Crossover,
    fir: FirSplit,
    until_hop: usize,
    held_hz: Option<[f64; SPLITS]>,
    bands: [[Vec<f32>; 2]; BANDS],
    tickle: f32,
}

impl Split {
    pub(crate) fn new(sample_rate: f64, room: usize) -> Split {
        let fir = FirSplit::new(sample_rate);
        Split {
            sample_rate,
            smooth: smoothing(SMOOTH_SECONDS, sample_rate) as f32,
            designed: None,
            filters: None,
            filters_settled_for: None,
            count: 0,
            crossover: Crossover::new(&fir),
            fir,
            until_hop: 0,
            held_hz: None,
            bands: std::array::from_fn(|_| std::array::from_fn(|_| vec![0.0; room])),
            tickle: DENORMAL,
        }
    }

    pub(crate) fn bands(&self) -> &[[Vec<f32>; 2]; BANDS] {
        &self.bands
    }

    pub(crate) fn bands_mut(&mut self) -> &mut [[Vec<f32>; 2]; BANDS] {
        &mut self.bands
    }

    pub(crate) fn restart(&mut self) {
        self.crossover.rest();
        self.until_hop = 0;
        self.held_hz = None;
    }

    pub(crate) fn renew(&mut self) {
        self.restart();
        self.filters = None;
        self.filters_settled_for = None;
        self.tickle = DENORMAL;
    }

    fn design(&mut self, voicing: Voicing) -> Filters {
        match self.designed {
            Some((designed_for, design)) if designed_for == voicing => design,
            _ => {
                let design = Filters::target(voicing, self.sample_rate);
                self.designed = Some((voicing, design));
                design
            }
        }
    }

    pub(crate) fn run(&mut self, l: &[f32], r: &[f32], voicing: Voicing) {
        let n = l.len();
        let count = voicing.count;
        if self.count != count {
            self.crossover.idle(count);
            self.count = count;
        }
        let design = self.design(voicing);
        if count == 1 {
            self.bands[0][0][..n].copy_from_slice(l);
            self.bands[0][1][..n].copy_from_slice(r);
            for (x, y) in l.iter().zip(r) {
                self.crossover.warm([*x, *y]);
            }
            self.filters = Some(design);
            self.filters_settled_for = Some(voicing);
            return;
        }
        let glide = f64::from(self.smooth);
        let mut filters = *self.filters.get_or_insert(design);
        let filters_move = self.filters_settled_for != Some(voicing) && !filters.still(&design, glide);
        if let Some(hz) = self.held_hz
            && (filters.linear || design.linear)
        {
            self.fir.design(&hz[..count - 1]);
        }
        let mut tickle = self.tickle;
        for i in 0..n {
            if self.until_hop == 0 {
                self.held_hz = Some(filters.hz);
                if filters.linear || design.linear {
                    self.fir.design(&filters.hz[..count - 1]);
                }
                self.until_hop = HOP;
            }
            self.until_hop -= 1;
            if filters_move {
                filters.toward(&design, glide);
            }
            tickle = -tickle;
            let split = self.crossover.split([l[i], r[i]], tickle, count, &filters, &self.fir);
            for (signal, split) in self.bands.iter_mut().zip(split).take(count) {
                signal[0][i] = split[0];
                signal[1][i] = split[1];
            }
        }
        self.tickle = tickle;
        self.filters = Some(filters);
        self.filters_settled_for = (!filters_move).then_some(voicing);
    }

}

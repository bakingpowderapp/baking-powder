use super::*;
use crate::params::{CombRead, Slope};
use crate::voice::Filters;
use sdk::dsp::Halfband;
use sdk::dsp::ladder::{Ladder, Taps, ladder_feedback};
use sdk::dsp::svf::{DRIVE_ENGAGE, Tap, svf_damping};
use std::f64::consts::{PI, SQRT_2};

const SVF18_CUTOFF_COMP: f64 = 1.357;
const SVF24_CUTOFF_COMP: f64 = 1.2465;
const COMB_MAX_GAIN: f64 = 0.99;
const COMB_MIN_PERIOD: f64 = 2.0;
const DRIVE_GAIN: f64 = 4.0;
const SHAPED_HALF_TURN: f64 = 1.13;
const FULL_VOICE: f64 = 1.0 / VOICE_TRIM;

#[derive(Clone, Copy, Debug)]
pub(super) struct Dial {
    pub cutoff: f64,
    pub k: f64,
    pub drive: f64,
    pub back: f64,
}

pub(super) fn feedback(mode: FilterMode, resonance: f64) -> f64 {
    match mode {
        FilterMode::Svf => svf_damping(resonance),
        FilterMode::Ladder => f64::from(ladder_feedback(resonance as f32)),
        FilterMode::Comb | FilterMode::CombNotch => resonance.clamp(0.0, COMB_MAX_GAIN),
    }
}

fn peak(k: f64) -> f64 {
    if k < SQRT_2 { 1.0 / (k * (1.0 - k * k / 4.0).sqrt()) } else { 1.0 }
}

fn peak_past(turn: f64, k: f64) -> f64 {
    let square = turn * turn;
    if k < SQRT_2 && square <= 1.0 - k * k / 2.0 { peak(k) } else { 1.0 / ((square - 1.0).powi(2) + k * k * square).sqrt() }
}

fn low_peak_in_phase(slope: Slope, driven: bool, [g, g2x, pole]: [f64; 3], k: f64) -> f64 {
    let (first_turn, second_turn) = match (driven, slope) {
        (false, Slope::Db12 | Slope::Db18) => return 1.0,
        (false, Slope::Db24) => {
            let turn = ((1.0 + SQRT_2 * g) / (g * (SQRT_2 + g))).max(1.0).sqrt();
            (turn, turn)
        }
        (true, Slope::Db12) => ((SHAPED_HALF_TURN / 4.0).tan() / g2x, 0.0),
        (true, Slope::Db18) => ((SHAPED_HALF_TURN * (1.0 - pole.asin() / PI) / 4.0).tan() / g2x, 0.0),
        (true, Slope::Db24) => {
            let lag = PI / SHAPED_HALF_TURN * g;
            let turn = (1.0 / (2.0 * lag * lag) - 1.0).max(0.0).sqrt().sqrt();
            let reached = g * turn;
            (reached / (1.0 + (1.0 + reached * reached).sqrt()) / g2x, turn)
        }
    };
    (peak_past(first_turn, k) / (1.0 + second_turn.powi(4)).sqrt()).max(1.0)
}

fn within_full_voice(sample: f64) -> f64 {
    let over = sample.abs() - FULL_VOICE;
    if over <= 0.0 { sample } else { (FULL_VOICE + FULL_VOICE * (over / FULL_VOICE).tanh()).copysign(sample) }
}

fn loop_peak(tap: Tap, slope: Slope, drive: f64, corner: [f64; 3], k: f64) -> f64 {
    let driven = drive > DRIVE_ENGAGE;
    let filter = match tap {
        Tap::Low => low_peak_in_phase(slope, driven, corner, k),
        Tap::Band => 1.0 / k,
        Tap::High => peak(k),
    };
    if driven { (1.0 + DRIVE_GAIN * drive) * filter } else { filter }
}

#[derive(Clone, Copy)]
pub(super) struct Law {
    pub mode: FilterMode,
    pub slope: Slope,
    pub shape: FilterShape,
    pub comb_read: CombRead,
    pub over: usize,
}

fn stereo<const N: usize>(out: [f64; N]) -> (f64, f64) {
    (out[0], out[N - 1])
}

pub(super) fn filter_stereo(v: &mut Voice, x_l: f64, x_r: f64, dial: Dial, amp: f64, c: &Block, sample_rate: f64) -> (f64, f64) {
    if v.filters.coupled && x_l.to_bits() != x_r.to_bits() {
        v.split();
    }
    let law = Law { mode: c.filter_mode, slope: c.filter_slope, shape: c.filter_shape, comb_read: c.filter_comb_read, over: c.filter_over };
    let f = &mut v.filters;
    if f.coupled { stereo(filtered(f, [x_l], dial, amp, law, c.filter_guard, sample_rate)) } else { stereo(filtered(f, [x_l, x_r], dial, amp, law, c.filter_guard, sample_rate)) }
}

fn oversampled(up: &mut [Halfband; 2], down: &mut [Halfband; 2], over: usize, x: f32, mut step: impl FnMut(f32) -> f32) -> f32 {
    let (a, b) = up[0].upsample(x);
    if over == 2 {
        return down[0].downsample(step(a), step(b));
    }
    let (a0, a1) = up[1].upsample(a);
    let (b0, b1) = up[1].upsample(b);
    let ya = down[1].downsample(step(a0), step(a1));
    let yb = down[1].downsample(step(b0), step(b1));
    down[0].downsample(ya, yb)
}

fn oversampled_lanes<const N: usize>(up: &mut [[Halfband; 2]; 2], down: &mut [[Halfband; 2]; 2], over: usize, x: [f32; N], mut step: impl FnMut([f32; N]) -> [f32; N]) -> [f32; N] {
    let first: [(f32, f32); N] = std::array::from_fn(|lane| up[lane][0].upsample(x[lane]));
    let (a, b) = (first.map(|(a, _)| a), first.map(|(_, b)| b));
    if over == 2 {
        let (ya, yb) = (step(a), step(b));
        return std::array::from_fn(|lane| down[lane][0].downsample(ya[lane], yb[lane]));
    }
    let second_a: [(f32, f32); N] = std::array::from_fn(|lane| up[lane][1].upsample(a[lane]));
    let second_b: [(f32, f32); N] = std::array::from_fn(|lane| up[lane][1].upsample(b[lane]));
    let (ya0, ya1) = (step(second_a.map(|(a0, _)| a0)), step(second_a.map(|(_, a1)| a1)));
    let ya: [f32; N] = std::array::from_fn(|lane| down[lane][1].downsample(ya0[lane], ya1[lane]));
    let (yb0, yb1) = (step(second_b.map(|(b0, _)| b0)), step(second_b.map(|(_, b1)| b1)));
    let yb: [f32; N] = std::array::from_fn(|lane| down[lane][1].downsample(yb0[lane], yb1[lane]));
    std::array::from_fn(|lane| down[lane][0].downsample(ya[lane], yb[lane]))
}

fn filtered<const N: usize>(f: &mut Filters, input: [f64; N], dial: Dial, amp: f64, law: Law, nyquist_guard: f64, sample_rate: f64) -> [f64; N] {
    let Dial { cutoff, k, drive, back } = dial;
    let over = law.over as f64;
    match law.mode {
        FilterMode::Ladder => {
            let g = f.ladder_corner.of((cutoff, law.over), |_| {
                let g_int = (PI * cutoff / (2.0 * sample_rate * over)).tan() as f32;
                g_int / (1.0 + g_int)
            });
            let tap: fn(Taps) -> f32 = match (law.shape, law.slope) {
                (FilterShape::BandPass, Slope::Db12) => Taps::bp12,
                (FilterShape::BandPass, Slope::Db18 | Slope::Db24) => Taps::bp24,
                (_, Slope::Db12) => |taps| taps.lp12,
                (_, Slope::Db18) => |taps| taps.lp18,
                (_, Slope::Db24) => |taps| taps.lp24,
            };
            let Filters { ladder, over_up, over_down, .. } = f;
            let (k, drive, back) = (k as f32, drive as f32, back as f32);
            let mut step = |x: [f32; N]| Ladder::tick2x_lanes(ladder, x, g, k, drive, back, tap).map(tap);
            let x: [f32; N] = std::array::from_fn(|channel| input[channel] as f32);
            let out = if law.over == 1 { step(x) } else { oversampled_lanes(over_up, over_down, law.over, x, step) };
            out.map(|out| f64::from(out) * amp)
        }
        FilterMode::Comb | FilterMode::CombNotch => {
            let sign = if law.mode == FilterMode::Comb { 1.0 } else { -1.0 };
            let len = Filters::comb_len(sample_rate);
            let period = (sample_rate / cutoff.max(1.0)).clamp(COMB_MIN_PERIOD, (len - 2) as f64);
            let Filters { comb, comb_at, comb_ap, .. } = f;
            let at = *comb_at % len;
            let allpass = law.comb_read == CombRead::Allpass;
            let out: [f64; N] = std::array::from_fn(|channel| {
                let line = &mut comb[channel];
                let delayed = if allpass {
                    let whole = (period - 0.5).floor();
                    let fraction = period - whole;
                    let eta = (1.0 - fraction) / (1.0 + fraction);
                    let back = at + len - whole as usize;
                    let (near, far) = (f64::from(line[back % len]), f64::from(line[(back - 1) % len]));
                    let read = eta * (near - comb_ap[channel]) + far;
                    comb_ap[channel] = read;
                    read
                } else {
                    let read = at as f64 + len as f64 - period;
                    let whole = read.floor();
                    let (first, next) = ((whole as usize) % len, (whole as usize + 1) % len);
                    f64::from(line[first]) + (read - whole) * f64::from(line[next] - line[first])
                };
                let y = input[channel] + sign * k * delayed;
                line[at] = y as f32;
                y * amp
            });
            *comb_at = (at + 1) % len;
            out
        }
        FilterMode::Svf => {
            let tap = match law.shape {
                FilterShape::LowPass => Tap::Low,
                FilterShape::BandPass => Tap::Band,
                FilterShape::HighPass => Tap::High,
            };
            let slope = if tap == Tap::Band { Slope::Db12 } else { law.slope };
            let comp = match slope {
                Slope::Db12 => 1.0,
                Slope::Db18 => SVF18_CUTOFF_COMP,
                Slope::Db24 => SVF24_CUTOFF_COMP,
            };
            let hz = if tap == Tap::High { cutoff / comp } else { (cutoff * comp).min(nyquist_guard) };
            let [g, g2x, pole] = f.svf_corner.of((hz, law.over), |_| {
                let rate = sample_rate * over;
                [(PI * hz / rate).tan(), (PI * hz / (2.0 * rate)).tan(), (-2.0 * PI * hz / rate).exp()]
            });
            let Filters { svf1, svf2, pole: poles, pole_hp, back: backs, over_up, over_down, .. } = f;
            let back = if back == 0.0 { 0.0 } else { back / loop_peak(tap, slope, drive, [g, g2x, pole], k) };
            std::array::from_fn(|channel| {
                let mut step = |x: f64| {
                    let fed = if back == 0.0 { x } else { x + back * within_full_voice(backs[channel]) };
                    let first = if drive > DRIVE_ENGAGE { svf1[channel].tick2x(fed, g2x, k, drive, tap) } else { svf1[channel].svf.tick(fed, g, k).at(tap) };
                    let out = match (slope, tap) {
                        (Slope::Db12, _) => first,
                        (Slope::Db18, Tap::High) => pole_hp[channel].tick(first, pole),
                        (Slope::Db18, _) => {
                            poles[channel] = (1.0 - pole) * first + pole * poles[channel];
                            poles[channel]
                        }
                        (Slope::Db24, _) => svf2[channel].tick(first, g, SQRT_2).at(tap),
                    };
                    backs[channel] = out;
                    out
                };
                let out = if law.over == 1 { step(input[channel]) } else { f64::from(oversampled(&mut over_up[channel], &mut over_down[channel], law.over, input[channel] as f32, |x| step(f64::from(x)) as f32)) };
                out * amp
            })
        }
    }
}

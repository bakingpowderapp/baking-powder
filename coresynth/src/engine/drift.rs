use super::motion::Tempo;
use super::{CENTS_PER_OCTAVE, OSCS, note_seed01};
use crate::params::Analog;
use sdk::dsp::sample_hold;
use sdk::dsp::unison::MAX_UNISON;

const DETUNE_SALT: u64 = 11;
const WANDER_SALT: u64 = 600;
const JITTER_SALT: u64 = 616;
const WIDTH_SALT: u64 = 632;
const CUTOFF_SALT: u64 = 633;
const LANE_SALT: u64 = 634;
const STEP: i64 = 64;
const SLOW_CELLS_PER_SECOND: [f64; 5] = [0.2, 0.4, 0.8, 1.6, 3.2];
const FAST_CELLS_PER_SECOND: [f64; 4] = [32.0, 64.0, 128.0, 256.0];
const BAND_POWER: f64 = 35.0 / 216.0;
const MOTION_POWER: f64 = 1.0 / 6.0;
const SEED_SPAN: f64 = 9_007_199_254_740_992.0;
const SEED_STRIDE: u64 = 0x9E37_79B9_7F4A_7C15;

#[derive(Clone, Copy, Debug)]
pub(super) struct DriftPlan {
    pub(super) on: bool,
    pub(super) glides: bool,
    pub(super) widens: bool,
    steps: bool,
    detune: f64,
    wander: f64,
    jitter: f64,
    cutoff: f64,
    width: f64,
    rate: f64,
}

impl DriftPlan {
    pub(super) fn of(analog: &Analog) -> DriftPlan {
        let analog = &analog.heard();
        let wander = depth(analog.wander, &SLOW_CELLS_PER_SECOND);
        let jitter = depth(analog.jitter, &FAST_CELLS_PER_SECOND);
        let cutoff = depth(analog.cutoff_wander, &SLOW_CELLS_PER_SECOND) / CENTS_PER_OCTAVE;
        let width = depth(analog.width_wander, &SLOW_CELLS_PER_SECOND);
        let steps = wander > 0.0 || jitter > 0.0 || cutoff > 0.0 || width > 0.0;
        DriftPlan { on: steps || analog.detune > 0.0, glides: cutoff > 0.0, widens: width > 0.0, steps, detune: analog.detune, wander, jitter, cutoff, width, rate: analog.wander_rate.clamp(Analog::MIN_RATE, Analog::MAX_RATE) }
    }

    pub(super) fn frames_left(&self, tempo: &Tempo, clock: i64) -> usize {
        if self.steps { left_in_step(tempo, clock) as usize } else { usize::MAX }
    }
}

fn depth(knob: f64, bands: &[f64]) -> f64 {
    if knob > 0.0 { knob * (MOTION_POWER / (BAND_POWER * bands.len() as f64)).sqrt() } else { 0.0 }
}

fn left_in_step(tempo: &Tempo, clock: i64) -> i64 {
    tempo.left_in_step(clock, STEP).unwrap_or_else(|| STEP - clock.rem_euclid(STEP))
}

fn step_start(tempo: &Tempo, clock: i64) -> i64 {
    clock + left_in_step(tempo, clock) - STEP
}

fn seed(osc: usize, slot: i64, salt: u64) -> u64 {
    ((note_seed01(osc as u8, slot, salt) * SEED_SPAN) as u64).wrapping_mul(SEED_STRIDE)
}

pub(super) fn lane_shift(lane: u64) -> i64 {
    if lane == 0 { 0 } else { seed(0, lane as i64, LANE_SALT) as i64 }
}

fn swell(seed: u64, at: f64) -> f64 {
    let cell = at.floor();
    let x = at - cell;
    let y = 1.0 - x;
    let held: [f64; 5] = std::array::from_fn(|k| sample_hold((cell as i64).wrapping_add(k as i64), seed));
    let blend = [y * y * y / 6.0, 2.0 / 3.0 - x * x * (2.0 - x) / 2.0, 2.0 / 3.0 - y * y * (2.0 - y) / 2.0, x * x * x / 6.0];
    blend.iter().zip(held.windows(2)).map(|(share, pair)| share * (pair[1] - pair[0])).sum()
}

fn roam(cells_per_second: &[f64], seed: u64, seconds: f64) -> f64 {
    let bands = cells_per_second.iter().zip(1u64..).map(|(cells, band)| {
        let seed = seed.wrapping_add(band.wrapping_mul(SEED_STRIDE));
        let offset = (seed >> 11) as f64 / SEED_SPAN;
        swell(seed, seconds * cells + offset)
    });
    bands.sum()
}

#[derive(Clone, Debug)]
pub(crate) struct Drift {
    pub(super) ratio: [[f64; MAX_UNISON]; OSCS],
    rand: [f64; OSCS],
    slot: i64,
    from: i64,
    into_step: i64,
    cutoff: f64,
    cutoff_per_frame: f64,
    width: [f64; OSCS],
    width_per_frame: [f64; OSCS],
}

impl Drift {
    pub(crate) fn new(slot: usize) -> Drift {
        Drift { ratio: [[1.0; MAX_UNISON]; OSCS], rand: [0.0; OSCS], slot: slot as i64, from: 0, into_step: 0, cutoff: 0.0, cutoff_per_frame: 0.0, width: [0.0; OSCS], width_per_frame: [0.0; OSCS] }
    }

    pub(super) fn draw(&mut self, slot: i64, midi: u8, seed_frame: i64, stack_at: usize, tempo: &Tempo, clock: i64) {
        self.rand = std::array::from_fn(|k| 2.0 * note_seed01(midi, seed_frame, DETUNE_SALT + (k + stack_at * OSCS) as u64) - 1.0);
        self.slot = slot;
        self.from = tempo.song(clock) - clock;
    }

    pub(super) fn refresh(&mut self, plan: &DriftPlan, unison: [usize; OSCS], tempo: &Tempo, clock: i64, sample_rate: f64) {
        let start = step_start(tempo, clock);
        let seconds = (start + self.from) as f64 / sample_rate;
        let paced = seconds * plan.rate;
        let slot = self.slot;
        self.into_step = clock - start;
        self.ratio = if plan.wander > 0.0 || plan.jitter > 0.0 {
            std::array::from_fn(|k| {
                let fixed = plan.detune * self.rand[k];
                let drifted = |u: usize| {
                    let slow = if plan.wander > 0.0 { plan.wander * roam(&SLOW_CELLS_PER_SECOND, seed(k, slot, WANDER_SALT + u as u64), paced) } else { 0.0 };
                    let fast = if plan.jitter > 0.0 { plan.jitter * roam(&FAST_CELLS_PER_SECOND, seed(k, slot, JITTER_SALT + u as u64), seconds) } else { 0.0 };
                    ((fixed + slow + fast) / CENTS_PER_OCTAVE).exp2()
                };
                let first = drifted(0);
                std::array::from_fn(|u| if u == 0 || u >= unison[k] { first } else { drifted(u) })
            })
        } else if plan.detune > 0.0 {
            std::array::from_fn(|k| [(plan.detune * self.rand[k] / CENTS_PER_OCTAVE).exp2(); MAX_UNISON])
        } else {
            [[1.0; MAX_UNISON]; OSCS]
        };
        let next = (start + STEP + self.from) as f64 / sample_rate * plan.rate;
        let glide = |depth: f64, seed: u64| {
            let [here, there] = [paced, next].map(|at| depth * roam(&SLOW_CELLS_PER_SECOND, seed, at));
            (here, (there - here) / STEP as f64)
        };
        (self.cutoff, self.cutoff_per_frame) = if plan.glides { glide(plan.cutoff, seed(0, slot, CUTOFF_SALT)) } else { (0.0, 0.0) };
        for k in 0..OSCS {
            (self.width[k], self.width_per_frame[k]) = if plan.widens { glide(plan.width, seed(k, slot, WIDTH_SALT)) } else { (0.0, 0.0) };
        }
    }

    pub(super) fn cutoff(&self, plan: &DriftPlan) -> f64 {
        if plan.glides { self.cutoff } else { 0.0 }
    }

    pub(super) fn cutoff_rise(&self, frame: usize) -> f64 {
        self.cutoff_per_frame * (self.into_step + frame as i64) as f64
    }

    pub(super) fn width(&self, plan: &DriftPlan, osc: usize) -> f64 {
        if plan.widens { self.width[osc] } else { 0.0 }
    }

    pub(super) fn width_rise(&self, osc: usize, frame: usize) -> f64 {
        self.width_per_frame[osc] * (self.into_step + frame as i64) as f64
    }
}

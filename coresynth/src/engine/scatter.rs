use super::{CENTS_PER_OCTAVE, OSCS, note_seed01, panned};
use crate::params::Analog;
use sdk::dsp::db_to_gain;
use sdk::dsp::envelope::{ATTACK, DECAY, RELEASE};

const SALT: u64 = 500;
const DRAWS: usize = 5 + OSCS;
const ARRIVED: f64 = 1e-6;

#[derive(Clone, Copy, Debug)]
pub(super) struct ScatterPlan {
    cutoff: f64,
    env: f64,
    width: f64,
    level: f64,
    pan: f64,
}

impl ScatterPlan {
    pub(super) fn of(analog: &Analog) -> ScatterPlan {
        let analog = &analog.heard();
        ScatterPlan { cutoff: analog.cutoff_scatter / CENTS_PER_OCTAVE, env: analog.env_scatter, width: analog.width_scatter, level: analog.level_scatter, pan: analog.pan_scatter }
    }
}

fn stretched(mut times: [f32; 5], octaves: f64) -> [f32; 5] {
    if octaves != 0.0 {
        let factor = octaves.exp2() as f32;
        for stage in [ATTACK, DECAY, RELEASE] {
            times[stage] *= factor;
        }
    }
    times
}

fn toward(at: f64, aim: f64, coeff: f64) -> f64 {
    let next = at + coeff * (aim - at);
    if (aim - next).abs() < ARRIVED { aim } else { next }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Offsets {
    cutoff: f64,
    width: [f64; OSCS],
    level: f64,
    pan: f64,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Scatter {
    drawn: Offsets,
    amp: f64,
    filter: f64,
    heard: Offsets,
    fresh: bool,
}

impl Scatter {
    pub(super) fn draw(&mut self, midi: u8, seed_frame: i64, stack_at: usize) {
        let units: [f64; DRAWS] = std::array::from_fn(|draw| 2.0 * note_seed01(midi, seed_frame, SALT + (stack_at * DRAWS + draw) as u64) - 1.0);
        let [cutoff, amp, filter, level, pan, width @ ..] = units;
        self.drawn = Offsets { cutoff, width, level, pan };
        self.amp = amp;
        self.filter = filter;
    }

    pub(crate) fn reset(&mut self) {
        self.fresh = true;
    }

    pub(super) fn aim(&mut self, plan: &ScatterPlan) -> Offsets {
        let drawn = &self.drawn;
        let aim = Offsets { cutoff: plan.cutoff * drawn.cutoff, width: drawn.width.map(|draw| plan.width * draw), level: plan.level * drawn.level, pan: plan.pan * drawn.pan };
        if self.fresh {
            self.fresh = false;
            self.heard = aim;
        }
        aim
    }

    pub(super) fn away(&self, aim: &Offsets) -> bool {
        self.heard != *aim
    }

    pub(super) fn glide(&mut self, aim: &Offsets, coeff: f64) -> bool {
        let heard = &mut self.heard;
        heard.cutoff = toward(heard.cutoff, aim.cutoff, coeff);
        heard.level = toward(heard.level, aim.level, coeff);
        heard.pan = toward(heard.pan, aim.pan, coeff);
        for (width, wanted) in heard.width.iter_mut().zip(aim.width) {
            *width = toward(*width, wanted, coeff);
        }
        self.away(aim)
    }

    pub(super) fn cutoff(&self) -> f64 {
        self.heard.cutoff
    }

    pub(super) fn width(&self, osc: usize) -> f64 {
        self.heard.width[osc]
    }

    pub(super) fn sides(&self, stack_pan: f64) -> (f64, f64) {
        let (left, right) = panned((stack_pan + self.heard.pan).clamp(-1.0, 1.0));
        let gain = if self.heard.level == 0.0 { 1.0 } else { db_to_gain(self.heard.level) };
        (left * gain, right * gain)
    }

    pub(super) fn amp_times(&self, plan: &ScatterPlan, times: [f32; 5]) -> [f32; 5] {
        stretched(times, plan.env * self.amp)
    }

    pub(super) fn filter_times(&self, plan: &ScatterPlan, times: [f32; 5]) -> [f32; 5] {
        stretched(times, plan.env * self.filter)
    }
}

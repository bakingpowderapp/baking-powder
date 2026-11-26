use crate::engine::{Drift, Heat, Scatter};
use crate::osc::StepRing;
use crate::params::{Motion, Params};
use sdk::dsp::{Halfband, OnePoleHp};
use sdk::dsp::envelope::Adsr;
use sdk::dsp::ladder::Ladder;
use sdk::dsp::noise::Noise;
use sdk::dsp::svf::{Shaped, Svf};
use sdk::dsp::unison::MAX_UNISON;

const COMB_LOWEST_HZ: f64 = 20.0;

#[derive(Clone, Copy, Debug, Default)]
pub struct Memo<K, V> {
    key: Option<K>,
    value: V,
}

impl<K: Copy + PartialEq, V: Copy> Memo<K, V> {
    pub fn of(&mut self, key: K, compute: impl FnOnce(K) -> V) -> V {
        if self.key != Some(key) {
            self.key = Some(key);
            self.value = compute(key);
        }
        self.value
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SubOsc {
    pub phase: f64,
    pub inc: f64,
    pub steps: StepRing,
}

#[derive(Clone, Debug, Default)]
pub struct NoiseVoice {
    pub noise: Noise,
    pub high: [Svf; 2],
    pub low: [Svf; 2],
}

impl NoiseVoice {
    fn reset_corners(&mut self) {
        for svf in self.high.iter_mut().chain(self.low.iter_mut()) {
            svf.reset();
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Moving {
    pub turns: [f64; Motion::LFOS],
    pub seeds: [u64; Motion::LFOS],
    pub spent: [bool; Motion::LFOS],
    pub rate_mod: [f64; Motion::LFOS],
    pub amount_mod: [f64; Motion::LFOS],
    pub drift: [f64; Motion::LFOS],
    pub keyed: [Memo<(f64, f64), f64>; Motion::LFOS],
    pub envs: [Adsr; Motion::ENVS],
}

impl Moving {
    pub fn start(&mut self, motion: &Motion, midi: u8, frame: i64) {
        for (k, lfo) in motion.lfos().iter().enumerate() {
            self.turns[k] = lfo.phase;
            self.spent[k] = false;
            self.seeds[k] = (u64::from(midi) << 56) ^ (frame as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (k as u64 + 1).wrapping_mul(0xD1B5_4A32_D192_ED03);
        }
    }

    pub fn trigger(&mut self) {
        for env in &mut self.envs {
            env.trigger();
        }
    }

    pub fn release(&mut self) {
        for env in &mut self.envs {
            env.release();
        }
    }

    pub fn rest(&mut self) {
        for env in &mut self.envs {
            env.rest();
        }
        self.rate_mod = [0.0; Motion::LFOS];
        self.amount_mod = [0.0; Motion::LFOS];
        self.drift = [0.0; Motion::LFOS];
    }
}

#[derive(Clone, Debug)]
pub struct Filters {
    pub svf1: [Shaped; 2],
    pub svf2: [Svf; 2],
    pub pole: [f64; 2],
    pub pole_hp: [OnePoleHp<f64>; 2],
    pub ladder: [Ladder; 2],
    pub ladder_corner: Memo<(f64, usize), f32>,
    pub svf_corner: Memo<(f64, usize), [f64; 3]>,
    pub back: [f64; 2],
    pub comb: [Vec<f32>; 2],
    pub comb_at: usize,
    pub comb_ap: [f64; 2],
    pub over_up: [[Halfband; 2]; 2],
    pub over_down: [[Halfband; 2]; 2],
    pub coupled: bool,
}

impl Filters {
    pub fn comb_len(sample_rate: f64) -> usize {
        (sample_rate / COMB_LOWEST_HZ) as usize + 4
    }

    fn new(sample_rate: f64) -> Filters {
        Filters {
            svf1: [Shaped::default(); 2],
            svf2: [Svf::default(); 2],
            pole: [0.0; 2],
            pole_hp: [OnePoleHp::default(); 2],
            ladder: [Ladder::default(); 2],
            ladder_corner: Memo::default(),
            svf_corner: Memo::default(),
            back: [0.0; 2],
            comb: std::array::from_fn(|_| vec![0.0; Filters::comb_len(sample_rate)]),
            comb_at: 0,
            comb_ap: [0.0; 2],
            over_up: [[Halfband::default(); 2]; 2],
            over_down: [[Halfband::default(); 2]; 2],
            coupled: true,
        }
    }

    pub fn reset(&mut self) {
        for line in &mut self.comb {
            line.fill(0.0);
        }
        self.comb_ap = [0.0; 2];
        for channel in 0..2 {
            self.svf1[channel].reset();
            self.svf2[channel].reset();
            self.ladder[channel].reset();
        }
        self.pole = [0.0; 2];
        self.pole_hp = [OnePoleHp::default(); 2];
        self.back = [0.0; 2];
        self.over_up = [[Halfband::default(); 2]; 2];
        self.over_down = [[Halfband::default(); 2]; 2];
        self.coupled = true;
    }

    pub fn split(&mut self) {
        let [left, right] = &mut self.comb;
        right.copy_from_slice(left);
        self.comb_ap[1] = self.comb_ap[0];
        self.svf1[1] = self.svf1[0];
        self.svf2[1] = self.svf2[0];
        self.pole[1] = self.pole[0];
        self.pole_hp[1] = self.pole_hp[0];
        self.ladder[1] = self.ladder[0];
        self.back[1] = self.back[0];
        self.over_up[1] = self.over_up[0];
        self.over_down[1] = self.over_down[0];
        self.coupled = false;
    }
}

#[derive(Clone, Debug)]
pub struct Voice {
    pub active: bool,
    pub midi: u8,
    pub glide_midi: f64,
    pub glide_from: f64,
    pub gliding: bool,
    pub vel: f64,
    pub vel_target: f64,
    pub cents: f64,
    pub transpose: f64,
    pub transpose_from: f64,
    pub modulation: f64,
    pub modulation_target: f64,
    pub mod_semitones: [f64; Params::OSCS],
    pub mod_eased: f64,
    pub cutoff_slew: f64,
    pub idle_at: Option<i64>,
    pub subs: [[SubOsc; MAX_UNISON]; Params::OSCS],
    pub pulse_width: Option<[f64; Params::OSCS]>,
    pub pulse_next: [f64; Params::OSCS],
    pub pulse_rise: [f64; Params::OSCS],
    pub noise: NoiseVoice,
    pub age: i64,
    pub env: Adsr,
    pub filter_env: Adsr,
    pub sync_hp: [[OnePoleHp<f64>; 2]; Params::OSCS],
    pub filters: Filters,
    pub force_fast: bool,
    pub drift: Drift,
    pub scatter: Scatter,
    pub heat: Heat,
    pub stack_at: usize,
    pub stack_of: usize,
    pub stack_cents: f64,
    pub stack_pan: f64,
    pub motion: Moving,
}

impl Voice {
    pub fn new(index: usize, sample_rate: f64) -> Voice {
        let mut voice = Voice {
            active: false,
            midi: 0,
            glide_midi: 0.0,
            glide_from: 0.0,
            gliding: false,
            vel: 1.0,
            vel_target: 1.0,
            cents: 0.0,
            transpose: 0.0,
            transpose_from: 0.0,
            modulation: 0.0,
            modulation_target: 0.0,
            mod_semitones: [0.0; Params::OSCS],
            mod_eased: 0.0,
            cutoff_slew: 0.0,
            idle_at: None,
            subs: [[SubOsc::default(); MAX_UNISON]; Params::OSCS],
            pulse_width: None,
            pulse_next: [0.0; Params::OSCS],
            pulse_rise: [0.0; Params::OSCS],
            noise: NoiseVoice::default(),
            age: 0,
            env: Adsr::default(),
            filter_env: Adsr::default(),
            sync_hp: [[OnePoleHp::default(); 2]; Params::OSCS],
            filters: Filters::new(sample_rate),
            force_fast: false,
            drift: Drift::new(index),
            scatter: Scatter::default(),
            heat: Heat::default(),
            stack_at: 0,
            stack_of: 1,
            stack_cents: 0.0,
            stack_pan: 0.0,
            motion: Moving::default(),
        };
        let seed = index as u32 + 1;
        voice.noise.noise.seed(seed.wrapping_mul(0x9E37_79B1) | 1, seed.wrapping_mul(0x85EB_CA77) | 1);
        voice
    }

    pub fn transpose_now(&self) -> f64 {
        let target = f64::from(self.midi);
        let span = self.glide_from - target;
        if self.gliding && span != 0.0 { self.transpose + (self.glide_midi - target) / span * (self.transpose_from - self.transpose) } else { self.transpose }
    }

    pub fn sounding(&self) -> bool {
        self.active && !self.heat.lingers()
    }

    pub fn end(&mut self, at: i64) {
        self.active = false;
        self.idle_at = Some(at);
        self.force_fast = false;
    }

    pub fn reset_filters(&mut self) {
        self.sync_hp = [[OnePoleHp::default(); 2]; Params::OSCS];
        self.filters.reset();
        self.scatter.reset();
        self.heat.reset();
    }

    pub fn split(&mut self) {
        self.filters.split();
    }

    pub fn flush_non_finite(&mut self) {
        for sub in self.subs.iter_mut().flatten() {
            sub.phase = 0.0;
            sub.steps.clear();
        }
        self.reset_filters();
        self.noise.reset_corners();
        self.env.rest();
        self.motion.rest();
        self.gliding = false;
        self.active = false;
        self.idle_at = None;
        self.force_fast = false;
    }
}

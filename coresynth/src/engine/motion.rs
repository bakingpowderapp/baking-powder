use super::*;
use crate::params::{Motion, Params, Shape, Target, Trigger};
use crate::sway::sway;
use sdk::face::Drawn;

const PITCH_SEMITONES: f64 = 12.0;
const PULSE_WIDTH_SWING: f64 = 0.45;
const RUN_SEED: u64 = 0x5EED;
const LAST_TURN: f64 = 1.0 - 1e-9;
const FALLBACK_BEATS_PER_SECOND: f64 = 2.0;
const RATE_OCTAVES: f64 = 10.0;
const PACED_MIN_HZ: f64 = 0.01;
const PACED_MAX_HZ: f64 = 2000.0;
const MIDDLE_C: f64 = 60.0;
const DRAWS_PER_BEAT: f64 = 960.0;

#[derive(Clone, Copy, Debug)]
pub(super) struct Tempo {
    frames_per_beat: f64,
    anchor: Option<(i64, f64, i64)>,
}

impl Tempo {
    pub(super) fn new(sample_rate: f64) -> Tempo {
        Tempo { frames_per_beat: sample_rate / FALLBACK_BEATS_PER_SECOND, anchor: None }
    }

    pub(super) fn at(clock: i64, frames_per_beat: f64, song: Option<(f64, i64)>, sample_rate: f64) -> Tempo {
        let frames_per_beat = if frames_per_beat > 0.0 { frames_per_beat } else { sample_rate / FALLBACK_BEATS_PER_SECOND };
        Tempo { frames_per_beat, anchor: song.map(|(beat, frame)| (clock, beat, frame)) }
    }

    fn beat(&self, clock: i64) -> Option<f64> {
        self.anchor.map(|(at, beat, _)| beat + (clock - at) as f64 / self.frames_per_beat)
    }

    pub(super) fn song(&self, clock: i64) -> i64 {
        self.anchor.map_or(clock, |(at, _, frame)| frame + clock - at)
    }

    pub(super) fn draw(&self, clock: i64) -> i64 {
        self.beat(clock).map_or(clock, |beat| (beat * DRAWS_PER_BEAT).round() as i64)
    }

    pub(super) fn left_in_step(&self, clock: i64, step: i64) -> Option<i64> {
        self.anchor.map(|(at, _, frame)| step - (frame + clock - at).rem_euclid(step))
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct LfoBlock {
    aimed: bool,
    shape: Shape,
    trigger: Trigger,
    target: Target,
    amount: f64,
    step: f64,
    key_track: f64,
    beats: Option<f64>,
    phase: f64,
    drawn: Drawn,
}

#[derive(Clone, Copy, Debug, Default)]
struct EnvBlock {
    aimed: bool,
    target: Target,
    amount: f64,
    velocity: f64,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct MotionBlock {
    pub(super) on: bool,
    pub(super) moves_levels: bool,
    pub(super) moves_widths: bool,
    lfos: [LfoBlock; Motion::LFOS],
    envs: [EnvBlock; Motion::ENVS],
    clock: i64,
    beat: Option<f64>,
    frames_per_beat: f64,
    step_floor: f64,
    step_ceiling: f64,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Sums {
    pub(super) pitch: [f64; Params::OSCS],
    pub(super) level: [f64; Params::OSCS],
    pub(super) pulse_width: [f64; Params::OSCS],
    pub(super) pan: f64,
    pub(super) cutoff: f64,
    pub(super) fm: [f64; Params::OSCS],
    pub(super) noise: f64,
    pub(super) amp: f64,
    pub(super) resonance: f64,
    lfo_rate: [f64; Motion::LFOS],
    lfo_amount: [f64; Motion::LFOS],
}

impl Sums {
    fn add(&mut self, target: Target, value: f64) {
        match target {
            Target::Off => {}
            Target::Lfo1Rate => self.lfo_rate[0] += value,
            Target::Lfo2Rate => self.lfo_rate[1] += value,
            Target::Lfo1Amount => self.lfo_amount[0] += value,
            Target::Lfo2Amount => self.lfo_amount[1] += value,
            Target::Pitch => {
                for pitch in &mut self.pitch {
                    *pitch += value * PITCH_SEMITONES;
                }
            }
            Target::Pitch1 => self.pitch[0] += value * PITCH_SEMITONES,
            Target::Pitch2 => self.pitch[1] += value * PITCH_SEMITONES,
            Target::Pitch3 => self.pitch[2] += value * PITCH_SEMITONES,
            Target::Pitch4 => self.pitch[3] += value * PITCH_SEMITONES,
            Target::Level1 => self.level[0] += value,
            Target::Level2 => self.level[1] += value,
            Target::Level3 => self.level[2] += value,
            Target::Level4 => self.level[3] += value,
            Target::PulseWidth1 => self.pulse_width[0] += value * PULSE_WIDTH_SWING,
            Target::PulseWidth2 => self.pulse_width[1] += value * PULSE_WIDTH_SWING,
            Target::PulseWidth3 => self.pulse_width[2] += value * PULSE_WIDTH_SWING,
            Target::PulseWidth4 => self.pulse_width[3] += value * PULSE_WIDTH_SWING,
            Target::Pan => self.pan += value,
            Target::Cutoff => self.cutoff += value * MOD_CUTOFF_OCTAVES,
            Target::Fm => {
                for fm in &mut self.fm {
                    *fm += value * FM_MAX_RADIANS / (2.0 * PI);
                }
            }
            Target::Fm1 => self.fm[0] += value * FM_MAX_RADIANS / (2.0 * PI),
            Target::Fm2 => self.fm[1] += value * FM_MAX_RADIANS / (2.0 * PI),
            Target::Fm3 => self.fm[2] += value * FM_MAX_RADIANS / (2.0 * PI),
            Target::Fm4 => self.fm[3] += value * FM_MAX_RADIANS / (2.0 * PI),
            Target::Noise => self.noise += value,
            Target::Amp => self.amp += value,
            Target::Resonance => self.resonance += value,
        }
    }
}

pub(super) fn ratio(semitones: f64) -> f64 {
    if semitones == 0.0 { 1.0 } else { (semitones / 12.0).exp2() }
}

impl MotionBlock {
    pub(super) fn of(motion: &Motion, clock: i64, tempo: Tempo, sample_rate: f64) -> MotionBlock {
        let frames_per_beat = tempo.frames_per_beat;
        let lfos = motion.lfos().map(|lfo| LfoBlock {
            aimed: lfo.aimed(),
            shape: lfo.shape,
            trigger: lfo.trigger,
            target: lfo.target,
            amount: lfo.amount,
            step: if lfo.sync { 1.0 / (lfo.division.beats() * frames_per_beat) } else { lfo.rate_hz / sample_rate },
            key_track: lfo.key_track,
            beats: lfo.sync.then(|| lfo.division.beats()),
            phase: lfo.phase,
            drawn: lfo.points,
        });
        let envs = motion.envs().map(|env| EnvBlock { aimed: env.aimed(), target: env.target, amount: env.amount, velocity: env.env.velocity });
        let beat = tempo.beat(clock);
        let moves = |targets: [Target; Params::OSCS]| targets.into_iter().any(|target| motion.moves(target));
        MotionBlock {
            on: motion.on(),
            moves_levels: moves([Target::Level1, Target::Level2, Target::Level3, Target::Level4]),
            moves_widths: moves([Target::PulseWidth1, Target::PulseWidth2, Target::PulseWidth3, Target::PulseWidth4]),
            lfos,
            envs,
            clock: tempo.song(clock),
            beat,
            frames_per_beat,
            step_floor: PACED_MIN_HZ / sample_rate,
            step_ceiling: PACED_MAX_HZ / sample_rate,
        }
    }

    pub(super) fn lift(&self, target: Target) -> f64 {
        let swung: f64 = self.lfos.iter().filter(|lfo| lfo.aimed && lfo.target == target).map(|lfo| lfo.amount.abs()).sum();
        let raised: f64 = self.envs.iter().filter(|env| env.aimed && env.target == target).map(|env| env.amount.max(0.0)).sum();
        swung + raised
    }

    pub(super) fn trace(&self, runs: &mut [Vec<f64>; Motion::LFOS], n: usize) {
        for (k, (lfo, run)) in self.lfos.iter().zip(runs).enumerate() {
            if lfo.aimed && lfo.trigger == Trigger::Run {
                for (i, wave) in run[..n].iter_mut().enumerate() {
                    *wave = self.run_wave(k, i, 0.0);
                }
            }
        }
    }

    fn run_wave(&self, k: usize, i: usize, drift: f64) -> f64 {
        let lfo = &self.lfos[k];
        let turns = match (lfo.beats, self.beat) {
            (Some(beats), Some(beat)) => (beat + i as f64 / self.frames_per_beat) / beats + lfo.phase,
            _ => (self.clock + i as i64) as f64 * lfo.step + lfo.phase,
        };
        sway(lfo.shape, turns + drift, RUN_SEED + k as u64, &lfo.drawn)
    }

    pub(super) fn sums(&self, v: &mut Voice, i: usize, runs: &[Vec<f64>; Motion::LFOS]) -> Sums {
        let levels = std::array::from_fn(|j| v.motion.envs[j].tick());
        let sums = self.at(v, i, levels, Some(runs));
        v.motion.rate_mod = sums.lfo_rate;
        v.motion.amount_mod = sums.lfo_amount;
        self.advance(v);
        sums
    }

    fn paced(&self, step: f64, keyed: f64, rate_mod: f64) -> f64 {
        if rate_mod == 0.0 && keyed == 1.0 { step } else { (step * keyed * (RATE_OCTAVES * rate_mod).exp2()).clamp(self.step_floor, self.step_ceiling) }
    }

    pub(super) fn peek(&self, v: &Voice) -> Sums {
        if self.on { self.at(v, 0, std::array::from_fn(|j| v.motion.envs[j].value()), None) } else { Sums::default() }
    }

    fn at(&self, v: &Voice, i: usize, levels: [f64; Motion::ENVS], runs: Option<&[Vec<f64>; Motion::LFOS]>) -> Sums {
        let mut sums = Sums::default();
        for (k, lfo) in self.lfos.iter().enumerate() {
            if !lfo.aimed {
                continue;
            }
            let drift = v.motion.drift[k];
            let wave = match (lfo.trigger, runs) {
                (Trigger::Run, Some(runs)) if drift == 0.0 => runs[k][i],
                (Trigger::Run, _) => self.run_wave(k, i, drift),
                (Trigger::Trig | Trigger::Once, _) => sway(lfo.shape, v.motion.turns[k], v.motion.seeds[k], &lfo.drawn),
            };
            let amount = if v.motion.amount_mod[k] == 0.0 { lfo.amount } else { (lfo.amount + v.motion.amount_mod[k]).clamp(-1.0, 1.0) };
            sums.add(lfo.target, amount * wave);
        }
        for (env, level) in self.envs.iter().zip(levels) {
            if env.aimed {
                sums.add(env.target, env.amount * level * (1.0 + env.velocity * (v.vel_target - 1.0)));
            }
        }
        sums
    }

    fn advance(&self, v: &mut Voice) {
        let pitch = v.glide_midi;
        for (k, lfo) in self.lfos.iter().enumerate() {
            if !lfo.aimed || v.motion.spent[k] {
                continue;
            }
            let keyed = if lfo.key_track == 0.0 { 1.0 } else { v.motion.keyed[k].of((pitch, lfo.key_track), |(pitch, key_track)| (key_track * (pitch - MIDDLE_C) / 12.0).exp2()) };
            if lfo.trigger == Trigger::Run {
                v.motion.drift[k] += self.paced(lfo.step, keyed, v.motion.rate_mod[k]) - lfo.step;
                continue;
            }
            let step = self.paced(lfo.step, keyed, v.motion.rate_mod[k]);
            let next = v.motion.turns[k] + step;
            if lfo.trigger == Trigger::Once && next >= lfo.phase + 1.0 {
                v.motion.turns[k] = lfo.phase + LAST_TURN;
                v.motion.spent[k] = true;
            } else {
                v.motion.turns[k] = next;
            }
        }
    }
}

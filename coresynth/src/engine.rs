use crate::osc::{Cycle, Edge, Stepped, edge_share, edge_steps, naive_wave, shaped};
use crate::params::{CombRead, Filter, FilterMode, FilterShape, GlideCurve, Modulation, Modulator, Mono, Motion, Params, Quantize, Slope, Target, WAVES, Wave};
use crate::sway;
use crate::voice::{Memo, Voice};
use sdk::dsp::envelope::{Adsr, Phase, RELEASE, Smoother};
use sdk::dsp::finite;
use sdk::dsp::unison::{MAX_UNISON, Spread};
use sdk::face::Drawn;
use sdk::notes::Articulation;
use std::f64::consts::{PI, SQRT_2};

mod block;
mod drift;
mod filters;
mod heat;
mod motion;
mod reach;
mod scatter;
mod voices;

pub(crate) use drift::Drift;
pub(crate) use heat::Heat;
pub(crate) use scatter::Scatter;

use drift::DriftPlan;
use filters::{Dial, feedback, filter_stereo};
use heat::HeatPlan;
use motion::{MotionBlock, Sums, Tempo};
use reach::Reach;
use scatter::ScatterPlan;

pub const VOICES: usize = 32;
const SPARES: usize = 8;
pub const CAPACITY: usize = 2048;
const OSCS: usize = Params::OSCS;
const VOICE_TRIM: f64 = 0.25;
const FAST_CUT_RELEASE: f32 = 0.004;
const GLIDE_TIME_CONSTANTS: f64 = 5.0;
const GLIDE_SETTLE_SEMIS: f64 = 1e-4;
const SEMITONES_PER_OCTAVE: f64 = 12.0;
const FM_MAX_RADIANS: f64 = 8.0;
const LINEAR_FM_PER_TURN: f64 = 2.0 * PI / FM_MAX_RADIANS;
const LINEAR_FM_FLOOR: f64 = 1e-9;
const NOISE_SMOOTH_SECONDS: f64 = 0.012;
const SYNC_DC_HZ: f64 = 5.0;
const VELOCITY_GLIDE_SECONDS: f64 = 0.005;
const INCREMENT_CEILING: f64 = 0.45;
const COPY_SALT: u64 = 100;
const FREE_COPY_SALT: u64 = 200;
const STACK_SALT: u64 = 300;
const CENTS_PER_OCTAVE: f64 = 1200.0;
const MOD_CUTOFF_OCTAVES: f64 = 5.0;
const MOD_SETTLE: f64 = 1e-4;
const BUTTERWORTH: f64 = SQRT_2;
pub const BEND_FRAMES: usize = 64;

#[derive(Clone, Copy, Debug)]
struct Onset {
    midi: u8,
    vel_norm: f64,
    cents: f64,
    modulation: f64,
    transpose: f64,
    from: f64,
    lagged: bool,
    glide: Option<(u8, f64)>,
    frame: i64,
}

#[derive(Clone, Copy, Debug)]
struct Held {
    midi: u8,
    velocity: u8,
    cents: f64,
    modulation: f64,
    transpose: f64,
}

#[derive(Debug)]
struct MonoState {
    stack: Vec<Held>,
    gated: bool,
    stale: bool,
    target_midi: u8,
    has_reference: bool,
}

impl MonoState {
    fn new() -> MonoState {
        MonoState { stack: Vec::with_capacity(128), gated: false, stale: false, target_midi: 60, has_reference: false }
    }

    fn clear(&mut self) {
        self.stack.clear();
        self.gated = false;
        self.stale = false;
        self.has_reference = false;
    }
}

#[derive(Clone, Copy, Debug)]
enum Glide {
    Eased(f64),
    Linear(f64),
    PerOctave(f64),
}

impl Glide {
    fn of(mono: &Mono, sample_rate: f64) -> Glide {
        let frames = mono.glide_ms.max(0.0) / 1000.0 * sample_rate;
        if frames <= 0.0 {
            return Glide::Eased(1.0);
        }
        match mono.curve {
            GlideCurve::Ease => Glide::Eased(1.0 - (-GLIDE_TIME_CONSTANTS / frames).exp()),
            GlideCurve::Linear => Glide::Linear(frames),
            GlideCurve::Octave => Glide::PerOctave(SEMITONES_PER_OCTAVE / frames),
        }
    }

    fn toward(self, at: f64, from: f64, target: f64) -> f64 {
        match self {
            Glide::Eased(alpha) => at + alpha * (target - at),
            Glide::Linear(frames) => {
                let step = (target - from).abs() / frames;
                at + (target - at).clamp(-step, step)
            }
            Glide::PerOctave(step) => at + (target - at).clamp(-step, step),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct NoiseSmooth {
    level: f64,
    tone: f64,
    width: f64,
    high_pass_hz: f64,
    low_pass_hz: f64,
}

#[derive(Debug)]
struct Curves {
    vel_amp: Vec<f64>,
    vel_filter: Vec<f64>,
    cutoff: Vec<f64>,
    resonance: Vec<f64>,
    k: Vec<f64>,
    drive: Vec<f64>,
    back: Vec<f64>,
    bends: [Vec<f64>; OSCS],
    runs: [Vec<f64>; Motion::LFOS],
    heat: heat::Dials,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Unison {
    count: usize,
    detune: f64,
    width: f64,
    alternate: bool,
    blend: f64,
}

#[derive(Clone, Copy, Debug)]
struct Bend {
    from: i64,
    len: usize,
    semitones: [f64; BEND_FRAMES],
}

impl Bend {
    fn at(&self, frame: i64) -> f64 {
        let Ok(k) = usize::try_from(frame - self.from) else {
            return 0.0;
        };
        let held = &self.semitones[..self.len];
        match held.get(k) {
            Some(semitones) => *semitones,
            None if k < self.len + BEND_FRAMES => held.last().copied().unwrap_or(0.0),
            None => 0.0,
        }
    }

    fn spent(&self, frame: i64) -> bool {
        frame - self.from >= (self.len + BEND_FRAMES) as i64
    }
}

#[derive(Clone, Copy, Debug)]
struct Block {
    glide: Glide,
    tune_ratio: [f64; OSCS],
    step_share: [f64; OSCS],
    unison: [usize; OSCS],
    noise: NoiseBlock,
    filter_on: bool,
    filter_mode: FilterMode,
    filter_slope: Slope,
    filter_shape: FilterShape,
    filter_comb_read: CombRead,
    filter_env_octaves: f64,
    filter_key_track: f64,
    resonance_velocity: f64,
    filter_fm_octaves: f64,
    cutoff_slew: Option<f64>,
    drift: DriftPlan,
    scatter: ScatterPlan,
    heat: HeatPlan,
    fm_turns: [f64; OSCS],
    fm_linear: [bool; OSCS],
    sync: [bool; OSCS],
    sync_phase: [f64; OSCS],
    sync_any: bool,
    sync_pole: f64,
    modulator: Option<usize>,
    carrier: [bool; OSCS],
    levels: [[f64; WAVES]; OSCS],
    shapes: [[Wave; WAVES]; OSCS],
    stepped: [Stepped; OSCS],
    edge: [Edge; OSCS],
    edged: [bool; OSCS],
    rings: [bool; OSCS],
    bent: [bool; OSCS],
    comp: [f64; OSCS],
    level: [f64; OSCS],
    pure: [bool; OSCS],
    sign: [f64; OSCS],
    pulse_width: [f64; OSCS],
    audible: [bool; OSCS],
    source_audible: bool,
    filter_over: usize,
    filter_guard: f64,
    mod_level: [f64; OSCS],
    mod_tune: [f64; OSCS],
    mod_tuned: bool,
    mod_quantize: Option<Quantize>,
    mod_lag_alpha: Option<f64>,
    mod_reach: Reach,
    filter_env_holds: bool,
    motion: MotionBlock,
}

#[derive(Clone, Copy, Debug, Default)]
struct NoiseBlock {
    on: bool,
    level: f64,
    high_g: Option<f64>,
    low_g: Option<f64>,
}

#[derive(Debug)]
pub struct Core {
    sample_rate: f64,
    params: Params,
    voices: Vec<Voice>,
    spread: [Spread; OSCS],
    spread_for: [Option<Unison>; OSCS],
    cycles: [Cycle; OSCS],
    cycles_of: [Option<Drawn>; OSCS],
    mono: MonoState,
    held: Vec<u8>,
    last_midi: Option<u8>,
    last_modulation: f64,
    last_transpose: f64,
    cutoff: Smoother,
    resonance: Smoother,
    drive: Smoother,
    back: Smoother,
    vel_amp: Smoother,
    vel_filter: Smoother,
    vel_glide_coeff: f64,
    noise_smooth: NoiseSmooth,
    last_noise_on: bool,
    last_filter_on: bool,
    last_filter_mode: FilterMode,
    sample_clock: i64,
    lane: u64,
    struck: Option<Articulation>,
    bends: [Option<Bend>; OSCS],
    curves: Curves,
    mix_l: Vec<f64>,
    mix_r: Vec<f64>,
    tempo: Tempo,
}

fn midi_to_hz(midi: f64) -> f64 {
    440.0 * ((midi - 69.0) / 12.0).exp2()
}

fn stepped(midi: f64, share: f64, transpose: f64) -> f64 {
    midi + (share - 1.0) * transpose
}

fn note_seed01(midi: u8, frame: i64, salt: u64) -> f64 {
    let mut z = u64::from(midi).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = z.wrapping_add((frame as u64).wrapping_mul(0xD1B5_4A32_D192_ED03));
    z ^= salt.wrapping_mul(0x2545_F491_4F6C_DD1D);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
}

fn tune_ratio(tune: f64, fine: f64) -> f64 {
    ((tune + fine / 100.0) / 12.0).exp2()
}

fn panned(pan: f64) -> (f64, f64) {
    (1.0 - pan.max(0.0), 1.0 + pan.min(0.0))
}

fn stack_cents(cents: f64, at: usize, of: usize) -> f64 {
    if of < 2 { 0.0 } else { cents * ((2 * at) as f64 / (of - 1) as f64 - 1.0) }
}

fn stack_pan(spread: f64, at: usize, of: usize) -> f64 {
    let middle = (of % 2 == 1).then_some(of / 2);
    if of < 2 || middle == Some(at) {
        return 0.0;
    }
    let turn = at - usize::from(middle.is_some_and(|middle| at > middle));
    if turn.is_multiple_of(2) { -spread } else { spread }
}

fn stack_turn(midi: u8, frame: i64, at: usize, osc: u64) -> f64 {
    if at == 0 { 0.0 } else { note_seed01(midi, frame, STACK_SALT + at as u64 * OSCS as u64 + osc) }
}

fn mod_levels(modulation: &Modulation) -> [f64; OSCS] {
    [modulation.level1, modulation.level2, 0.0, 0.0]
}

fn level_target(osc: usize) -> Target {
    [Target::Level1, Target::Level2, Target::Level3, Target::Level4][osc]
}

fn voice_shifts(voice: &Voice, mod_tune: &[f64; OSCS], quantized: bool) -> [f64; OSCS] {
    let shift = ((voice.cents + voice.stack_cents) / CENTS_PER_OCTAVE).exp2();
    std::array::from_fn(|osc| {
        let semitones = mod_tune[osc];
        if semitones == 0.0 {
            shift
        } else if quantized {
            shift * (voice.mod_semitones[osc] / SEMITONES_PER_OCTAVE).exp2()
        } else {
            shift * (semitones * voice.modulation / SEMITONES_PER_OCTAVE).exp2()
        }
    })
}

fn advanced(next: f64) -> f64 {
    if next >= 1.0 { next - 1.0 } else { next }
}

fn corner(hz: f64, nyquist_guard: f64, sample_rate: f64) -> f64 {
    (PI * hz.clamp(Filter::MIN_HZ, nyquist_guard) / sample_rate).tan()
}

fn silent(env: &Adsr) -> bool {
    env.phase == Phase::Idle || (env.phase == Phase::Sustain && env.value() <= 0.0)
}

fn shape_sum(shapes: &[Wave; WAVES], levels: &[f64; WAVES], cycle: &Cycle, phase: f64, dt: f64, pulse_width: f64, share: f64) -> f64 {
    let mut s = 0.0;
    for (wave, level) in shapes.iter().zip(levels) {
        if *level > 0.0 {
            s += shaped(*wave, phase, dt, pulse_width, share, cycle) * level;
        }
    }
    s
}

fn naive_shapes(shapes: &[Wave; WAVES], levels: &[f64; WAVES], cycle: &Cycle, phase: f64, pulse_width: f64) -> f64 {
    let mut s = 0.0;
    for (wave, level) in shapes.iter().zip(levels) {
        if *level > 0.0 {
            s += naive_wave(*wave, phase, pulse_width, cycle) * level;
        }
    }
    s
}

#[inline(never)]
fn move_widths(v: &mut Voice, c: &Block, audible: &[bool; OSCS], wanted: [f64; OSCS]) -> ([f64; OSCS], [f64; OSCS]) {
    let (from, to) = v.pulse_width.map_or((wanted, wanted), |from| (from, v.pulse_next));
    for osc in 0..OSCS {
        let jumped = to[osc] != from[osc] && v.pulse_rise[osc] == 0.0;
        if jumped && c.edged[osc] && (audible[osc] || c.modulator == Some(osc)) {
            for copy in &mut v.subs[osc][..c.unison[osc]] {
                copy.steps.widen(c.edge[osc], copy.phase, &c.stepped[osc], from[osc], to[osc]);
            }
        }
        let step = wanted[osc] - to[osc];
        v.pulse_rise[osc] = if c.edged[osc] && c.stepped[osc].glides(step) { step } else { 0.0 };
    }
    v.pulse_width = Some(to);
    v.pulse_next = wanted;
    (to, v.pulse_rise)
}

pub(crate) fn mix_compensation(levels: [f64; WAVES]) -> f64 {
    let energy: f64 = levels.iter().map(|l| l * l).sum();
    if energy > 0.0 { 1.0 / energy.sqrt() } else { 0.0 }
}

impl Core {
    pub fn new(sample_rate: f64, params: Params) -> Core {
        sway::ready();
        let sr = sample_rate as f32;
        let filter = params.filter;
        let mut core = Core {
            sample_rate,
            params,
            voices: (0..VOICES + SPARES).map(|index| Voice::new(index, sample_rate)).collect(),
            spread: [Spread::default(); OSCS],
            spread_for: [None; OSCS],
            cycles: std::array::from_fn(|_| Cycle::flat()),
            cycles_of: [None; OSCS],
            mono: MonoState::new(),
            held: Vec::with_capacity(128),
            last_midi: None,
            last_modulation: 0.0,
            last_transpose: 0.0,
            cutoff: Smoother::fast(filter.cutoff_hz as f32, sr),
            resonance: Smoother::new(filter.resonance as f32, sr),
            drive: Smoother::new(filter.drive as f32, sr),
            back: Smoother::new(filter.feedback as f32, sr),
            vel_amp: Smoother::new(params.amp.velocity as f32, sr),
            vel_filter: Smoother::new(filter.env.velocity as f32, sr),
            vel_glide_coeff: 1.0 - (-1.0 / (VELOCITY_GLIDE_SECONDS * sample_rate)).exp(),
            noise_smooth: NoiseSmooth::default(),
            last_noise_on: false,
            last_filter_on: false,
            last_filter_mode: FilterMode::Svf,
            sample_clock: 0,
            lane: 0,
            struck: None,
            bends: [None; OSCS],
            curves: Curves { vel_amp: vec![0.0; CAPACITY], vel_filter: vec![0.0; CAPACITY], cutoff: vec![0.0; CAPACITY], resonance: vec![0.0; CAPACITY], k: vec![0.0; CAPACITY], drive: vec![0.0; CAPACITY], back: vec![0.0; CAPACITY], bends: std::array::from_fn(|_| vec![1.0; CAPACITY]), runs: std::array::from_fn(|_| vec![0.0; CAPACITY]), heat: heat::Dials::new(sample_rate, &params.analog) },
            mix_l: vec![0.0; CAPACITY],
            mix_r: vec![0.0; CAPACITY],
            tempo: Tempo::new(sample_rate),
        };
        core.respread();
        core
    }

    pub fn set_params(&mut self, params: Params) {
        self.params = params;
        self.respread();
    }

    pub fn clock(&mut self, frames_per_beat: f64, song: Option<(f64, i64)>) {
        self.tempo = Tempo::at(self.sample_clock, frames_per_beat, song, self.sample_rate);
    }

    pub fn set_lane(&mut self, lane: u64) {
        self.lane = lane;
    }

    fn retrace(&mut self) {
        let Core { params, cycles, cycles_of, .. } = self;
        for ((cycle, traced), osc) in cycles.iter_mut().zip(cycles_of).zip(params.oscs()) {
            if osc.shapes.contains(&Wave::Drawn) && *traced != Some(osc.points) {
                *traced = Some(osc.points);
                cycle.trace(&osc.points);
            }
        }
    }

    fn respread(&mut self) {
        let Core { params, spread, spread_for, .. } = self;
        for ((spread, built), osc) in spread.iter_mut().zip(spread_for).zip(params.oscs()) {
            let unison = Some(Unison { count: osc.unison, detune: osc.detune, width: osc.width, alternate: osc.alternate, blend: osc.blend });
            if *built == unison {
                continue;
            }
            *built = unison;
            if osc.alternate {
                spread.alternate(osc.unison, osc.detune, osc.width, osc.blend);
            } else {
                spread.fill(osc.unison, osc.detune, osc.width, osc.blend);
            }
        }
    }

    pub fn bend(&mut self, osc: usize, semitones: &[f64]) {
        let from = self.sample_clock;
        let Some(slot) = self.bends.get_mut(osc) else {
            return;
        };
        if slot.as_ref().is_none_or(|bend| bend.from != from) {
            *slot = Some(Bend { from, len: 0, semitones: [0.0; BEND_FRAMES] });
        }
        if let Some(bend) = slot {
            let len = semitones.len().min(BEND_FRAMES);
            for (held, added) in bend.semitones.iter_mut().zip(&semitones[..len]) {
                *held += added;
            }
            bend.len = bend.len.max(len);
        }
    }

    pub fn render(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        let n = out_l.len().min(out_r.len());
        let whole = self.frames_to_wander().min(CAPACITY);
        if n > whole {
            let (l_a, l_b) = out_l.split_at_mut(whole);
            let (r_a, r_b) = out_r.split_at_mut(whole);
            self.render(l_a, r_a);
            self.render(l_b, r_b);
            return;
        }
        let block = self.prepare(n);
        self.mix_l[..n].fill(0.0);
        self.mix_r[..n].fill(0.0);
        for vi in 0..self.voices.len() {
            if self.voices[vi].active {
                self.render_voice(vi, n, &block);
            }
        }
        for i in 0..n {
            out_l[i] = finite((self.mix_l[i] * VOICE_TRIM) as f32);
            out_r[i] = finite((self.mix_r[i] * VOICE_TRIM) as f32);
        }
        self.sample_clock += n as i64;
    }

    fn render_voice(&mut self, vi: usize, n: usize, c: &Block) {
        let Core { voices, curves, spread, cycles, mix_l, mix_r, sample_rate, vel_glide_coeff, sample_clock, .. } = self;
        let sample_rate = *sample_rate;
        let block_start = *sample_clock;
        let vel_glide_coeff = *vel_glide_coeff;
        let v = &mut voices[vi];
        let aim = v.scatter.aim(&c.scatter);
        let mut scattering = v.scatter.away(&aim);
        let (mut stack_l, mut stack_r) = v.scatter.sides(v.stack_pan);
        let mut cutoff_offset = v.drift.cutoff(&c.drift) + v.scatter.cutoff();
        let widths: [f64; OSCS] = std::array::from_fn(|osc| c.pulse_width[osc] + v.drift.width(&c.drift, osc));
        let width = |osc: usize, swing: f64| if c.pure[osc] { (widths[osc] + swing).clamp(0.05, 0.95) } else { c.pulse_width[osc] };
        let mod_hz: [f64; OSCS] = std::array::from_fn(|osc| if c.mod_tune[osc] != 0.0 { midi_to_hz(stepped(f64::from(v.midi), c.step_share[osc], v.transpose)) / sample_rate } else { 0.0 });
        let mut modulation = v.modulation;
        let mut lagging = c.mod_lag_alpha.is_some() && v.modulation != v.modulation_target;
        let mut easing = lagging || v.mod_eased != modulation;
        let mut reached = c.mod_reach.at(v.mod_eased);
        let aims: [f64; OSCS] = c.mod_quantize.map_or([0.0; OSCS], |quantize| c.mod_tune.map(|tune| quantize.snap(tune * v.modulation_target)));
        let scale = |osc: usize, modulation: f64, swing: f64| c.comp[osc] * (c.level[osc] + c.mod_level[osc] * modulation + swing).clamp(0.0, 1.0);
        let levelled = |osc_scale: [f64; OSCS], modulation: f64, m: &Sums| -> ([f64; OSCS], [bool; OSCS]) {
            let scales: [f64; OSCS] = std::array::from_fn(|osc| if m.level[osc] != 0.0 { scale(osc, modulation, m.level[osc]) } else { osc_scale[osc] });
            (scales, std::array::from_fn(|osc| c.audible[osc] && scales[osc] > 0.0))
        };
        let lagless = c.mod_lag_alpha.is_none();
        let mut level_mod = if lagless { v.mod_eased } else { modulation };
        let mut osc_scale: [f64; OSCS] = std::array::from_fn(|osc| scale(osc, level_mod, 0.0));
        let mut shifts = voice_shifts(v, &c.mod_tune, c.mod_quantize.is_some());
        let mut m = Sums::default();
        let (mut scales, mut audible) = levelled(osc_scale, level_mod, &m);
        let mut pulse_width: [f64; OSCS] = std::array::from_fn(|osc| width(osc, v.scatter.width(osc) + reached.width));
        let widening = v.pulse_width != Some(pulse_width) || v.pulse_next != pulse_width || c.motion.moves_widths || c.drift.widens || (easing && c.mod_reach.width != 0.0);
        pulse_width = v.pulse_width.unwrap_or(pulse_width);
        let mut rise = [0.0; OSCS];
        let unison_mod = c.unison[c.modulator.unwrap_or(1)];
        let (mut glided, mut pitched, mut lifted, mut resonated) = (Memo::default(), Memo::default(), Memo::default(), Memo::default());
        for i in 0..n {
            let lagged = lagging;
            if lagging {
                let alpha = c.mod_lag_alpha.unwrap_or(1.0);
                v.modulation += alpha * (v.modulation_target - v.modulation);
                if (v.modulation_target - v.modulation).abs() < MOD_SETTLE {
                    v.modulation = v.modulation_target;
                    lagging = false;
                }
                if c.mod_quantize.is_some() {
                    for (semitones, aim) in v.mod_semitones.iter_mut().zip(aims) {
                        *semitones = if lagging { *semitones + alpha * (aim - *semitones) } else { aim };
                    }
                }
                modulation = v.modulation;
                level_mod = modulation;
                osc_scale = std::array::from_fn(|osc| scale(osc, modulation, 0.0));
                if c.mod_tuned {
                    shifts = voice_shifts(v, &c.mod_tune, c.mod_quantize.is_some());
                    if !v.gliding {
                        for osc in (0..OSCS).filter(|osc| c.mod_tune[*osc] != 0.0) {
                            let base = mod_hz[osc] * c.tune_ratio[osc];
                            for u in 0..c.unison[osc] {
                                let inc = if c.drift.on { base * v.drift.ratio[osc][u] } else { base } * shifts[osc];
                                v.subs[osc][u].inc = inc * spread[osc].detune[u];
                            }
                        }
                    }
                }
            }
            let eased = easing && lagless;
            if easing {
                v.mod_eased += vel_glide_coeff * (modulation - v.mod_eased);
                if !lagging && (modulation - v.mod_eased).abs() < MOD_SETTLE {
                    v.mod_eased = modulation;
                    easing = false;
                }
                reached = c.mod_reach.at(v.mod_eased);
            }
            if eased {
                level_mod = v.mod_eased;
                osc_scale = std::array::from_fn(|osc| scale(osc, level_mod, 0.0));
            }
            if c.motion.on {
                m = c.motion.sums(v, i, &curves.runs);
            }
            if lagged || eased || c.motion.moves_levels {
                (scales, audible) = levelled(osc_scale, level_mod, &m);
            }
            let scattered = scattering;
            let swung: [f64; OSCS] = std::array::from_fn(|osc| if c.drift.widens { m.pulse_width[osc] + v.drift.width_rise(osc, i) } else { m.pulse_width[osc] });
            if scattering {
                scattering = v.scatter.glide(&aim, vel_glide_coeff);
                (stack_l, stack_r) = v.scatter.sides(v.stack_pan);
                cutoff_offset = v.drift.cutoff(&c.drift) + v.scatter.cutoff();
            }
            if scattered || widening {
                let wanted: [f64; OSCS] = std::array::from_fn(|osc| width(osc, v.scatter.width(osc) + swung[osc] + reached.width));
                if v.pulse_width != Some(wanted) || v.pulse_next != wanted {
                    (pulse_width, rise) = move_widths(v, c, &audible, wanted);
                }
            }
            if v.gliding {
                let target = f64::from(v.midi);
                v.glide_midi = c.glide.toward(v.glide_midi, v.glide_from, target);
                if (v.glide_midi - target).abs() < GLIDE_SETTLE_SEMIS {
                    v.glide_midi = target;
                    v.gliding = false;
                }
                let transpose = v.transpose_now();
                for osc in 0..OSCS {
                    let glide_hz = glided.of(stepped(v.glide_midi, c.step_share[osc], transpose), |midi| midi_to_hz(midi) / sample_rate);
                    let base = glide_hz * c.tune_ratio[osc];
                    for u in 0..c.unison[osc] {
                        let inc = if c.drift.on { base * v.drift.ratio[osc][u] } else { base } * shifts[osc];
                        v.subs[osc][u].inc = inc * spread[osc].detune[u];
                    }
                }
            }
            if v.vel != v.vel_target {
                v.vel += vel_glide_coeff * (v.vel_target - v.vel);
                if (v.vel_target - v.vel).abs() < 1e-4 {
                    v.vel = v.vel_target;
                }
            }
            let mut pitch = [1.0; OSCS];
            for (osc, ratio) in pitch.iter_mut().enumerate() {
                if c.bent[osc] {
                    *ratio *= curves.bends[osc][i];
                }
                *ratio *= pitched.of(m.pitch[osc], motion::ratio);
            }
            let vel_amp_sens = curves.vel_amp[i];
            let amp_motion = (1.0 + m.amp).max(0.0);
            let e = v.env.tick() * (1.0 + vel_amp_sens * (v.vel - 1.0)) * amp_motion;
            let fe = if c.filter_on { v.filter_env.tick() } else { 0.0 };
            let mut x_l = 0.0;
            let mut x_r = 0.0;
            let mut mod_values = [0.0; MAX_UNISON];
            let mut mod_mean = 0.0;
            let mut master_wrapped = false;
            let mut sync_tau = 0.0;
            let mut master_dt = 0.0;
            if let Some(mo) = c.modulator {
                let mut a = 0.0;
                let mut b = 0.0;
                for (u, modulator) in mod_values.iter_mut().enumerate().take(unison_mod) {
                    let sub = &mut v.subs[mo][u];
                    let ph = sub.phase;
                    let dt = (sub.inc * pitch[mo]).min(INCREMENT_CEILING);
                    let share = edge_share(dt);
                    let s = (shape_sum(&c.shapes[mo], &c.levels[mo], &cycles[mo], ph, dt, pulse_width[mo], share) + sub.steps.edged(c.edge[mo], ph, dt, share, &c.stepped[mo], pulse_width[mo], rise[mo])) * c.sign[mo];
                    *modulator = s * c.comp[mo];
                    if u == 0 {
                        master_dt = dt;
                        if c.sync_any && ph + dt >= 1.0 {
                            master_wrapped = true;
                            sync_tau = (1.0 - ph) / dt;
                        }
                    }
                    a += s * spread[mo].gain_l[u];
                    b += s * spread[mo].gain_r[u];
                    sub.phase = advanced(ph + dt);
                }
                mod_mean = mod_values[..unison_mod].iter().sum::<f64>() / unison_mod as f64;
                if audible[mo] {
                    x_l += a * scales[mo];
                    x_r += b * scales[mo];
                }
            }
            for osc in 0..OSCS {
                if !audible[osc] || Some(osc) == c.modulator {
                    continue;
                }
                let mut a = 0.0;
                let mut b = 0.0;
                let synced = c.sync[osc];
                if c.carrier[osc] {
                    let fm_turns = c.fm_turns[osc] + m.fm[osc];
                    let (fm_turns, deviation) = if c.fm_linear[osc] { (0.0, fm_turns * LINEAR_FM_PER_TURN) } else { (fm_turns, 0.0) };
                    for (u, modulator) in (0..c.unison[osc]).zip(mod_values[..unison_mod].iter().copied().cycle()) {
                        let sub = &mut v.subs[osc][u];
                        let ph = sub.phase;
                        let dt = if c.fm_linear[osc] { (sub.inc * pitch[osc] * (1.0 + deviation * modulator)).clamp(LINEAR_FM_FLOOR, INCREMENT_CEILING) } else { (sub.inc * pitch[osc]).min(INCREMENT_CEILING) };
                        let sp = if fm_turns != 0.0 {
                            let shifted = ph + fm_turns * modulator;
                            shifted - shifted.floor()
                        } else {
                            ph
                        };
                        let share = if !c.edged[osc] {
                            0.0
                        } else if synced {
                            1.0
                        } else {
                            edge_share(dt)
                        };
                        let edge = c.edge[osc];
                        let mut s = shape_sum(&c.shapes[osc], &c.levels[osc], &cycles[osc], sp, dt, pulse_width[osc], share);
                        if !synced {
                            sub.steps.settle(edge.steps(edge_steps(c.stepped[osc].net(), share)), dt);
                        }
                        if c.rings[osc] {
                            s += if c.edged[osc] { share } else { 1.0 } * f64::from(sub.steps.pop());
                        }
                        let s = s * c.sign[osc];
                        a += s * spread[osc].gain_l[u];
                        b += s * spread[osc].gain_r[u];
                        let stepped = &c.stepped[osc];
                        let reset = synced && master_wrapped;
                        let to = if reset { sync_tau } else { 1.0 };
                        if share > 0.0 {
                            sub.steps.edges(edge, ph, dt, 0.0, to, stepped, pulse_width[osc], rise[osc]);
                        }
                        if reset {
                            let reset_to = c.sync_phase[osc];
                            let offset = fm_turns * modulator;
                            let mut wrapped = ph + sync_tau * dt + offset;
                            wrapped -= wrapped.floor();
                            let landing = if fm_turns != 0.0 { (reset_to + offset).rem_euclid(1.0) } else { reset_to };
                            let width = pulse_width[osc] + rise[osc] * sync_tau;
                            let before = naive_shapes(&c.shapes[osc], &c.levels[osc], &cycles[osc], wrapped, width);
                            let after = naive_shapes(&c.shapes[osc], &c.levels[osc], &cycles[osc], landing, width);
                            sub.steps.stamp(edge, (after - before) as f32, (1.0 - sync_tau) as f32);
                            let wraps = if c.edged[osc] { (dt / master_dt).floor() } else { 0.0 };
                            let mut steps = edge.steps(edge_steps(stepped.net(), share) * wraps);
                            steps[edge as usize] += after - before;
                            sub.steps.settle(steps, master_dt);
                            if c.edged[osc] {
                                sub.steps.edges(edge, reset_to, dt, sync_tau, 1.0, stepped, pulse_width[osc], rise[osc]);
                            }
                            sub.phase = advanced(reset_to + (1.0 - sync_tau) * dt);
                        } else {
                            sub.phase = advanced(ph + dt);
                        }
                    }
                    if synced {
                        a = v.sync_hp[osc][0].tick(a, c.sync_pole);
                        b = v.sync_hp[osc][1].tick(b, c.sync_pole);
                    }
                } else {
                    for u in 0..c.unison[osc] {
                        let sub = &mut v.subs[osc][u];
                        let ph = sub.phase;
                        let dt = (sub.inc * pitch[osc]).min(INCREMENT_CEILING);
                        let share = edge_share(dt);
                        let s = (shape_sum(&c.shapes[osc], &c.levels[osc], &cycles[osc], ph, dt, pulse_width[osc], share) + sub.steps.edged(c.edge[osc], ph, dt, share, &c.stepped[osc], pulse_width[osc], rise[osc])) * c.sign[osc];
                        a += s * spread[osc].gain_l[u];
                        b += s * spread[osc].gain_r[u];
                        sub.phase = advanced(ph + dt);
                    }
                }
                x_l += a * scales[osc];
                x_r += b * scales[osc];
            }
            if c.noise.on {
                let gain = (c.noise.level + m.noise).max(0.0);
                let noise = v.noise.noise.tick();
                let (mut l, mut r) = (noise.l, noise.r);
                if let Some(g) = c.noise.high_g {
                    l = v.noise.high[0].tick(l, g, BUTTERWORTH).hp;
                    r = v.noise.high[1].tick(r, g, BUTTERWORTH).hp;
                }
                if let Some(g) = c.noise.low_g {
                    l = v.noise.low[0].tick(l, g, BUTTERWORTH).lp;
                    r = v.noise.low[1].tick(r, g, BUTTERWORTH).lp;
                }
                x_l += l * gain;
                x_r += r * gain;
            }
            if c.heat.pre {
                (x_l, x_r) = v.heat.pre(&c.heat, &curves.heat, i, x_l, x_r);
            }
            let (mut now_l, mut now_r) = if c.filter_on {
                let key = (f64::from(v.midi) - 60.0) / 12.0;
                let felt = fe * (1.0 + curves.vel_filter[i] * (v.vel_target - 1.0));
                let moved = m.cutoff + reached.cutoff;
                let octaves = match c.cutoff_slew {
                    Some(alpha) => {
                        v.cutoff_slew += alpha * (felt * c.filter_env_octaves + moved - v.cutoff_slew);
                        v.cutoff_slew + key * c.filter_key_track + c.filter_fm_octaves * mod_mean + cutoff_offset
                    }
                    None => felt * c.filter_env_octaves + key * c.filter_key_track + c.filter_fm_octaves * mod_mean + moved + cutoff_offset,
                };
                let octaves = if c.drift.glides { octaves + v.drift.cutoff_rise(i) } else { octaves };
                let k = if c.resonance_velocity == 0.0 && m.resonance == 0.0 {
                    curves.k[i]
                } else {
                    let resonance = (curves.resonance[i] + c.resonance_velocity * v.vel_target + m.resonance).clamp(0.0, 1.0);
                    resonated.of(resonance, |resonance| feedback(c.filter_mode, resonance))
                };
                let dial = Dial { cutoff: (curves.cutoff[i] * lifted.of(octaves, f64::exp2)).clamp(Filter::MIN_CUTOFF_HZ, c.filter_guard), k, drive: curves.drive[i], back: curves.back[i] };
                filter_stereo(v, x_l, x_r, dial, e, c, sample_rate)
            } else if e > 0.0 && c.source_audible {
                (x_l * e, x_r * e)
            } else {
                (0.0, 0.0)
            };
            if c.heat.after {
                (now_l, now_r) = v.heat.after(&c.heat, &curves.heat, i, now_l, now_r);
            }
            if now_l.is_finite() && now_r.is_finite() {
                let (gain_l, gain_r) = panned((m.pan + reached.pan).clamp(-1.0, 1.0));
                mix_l[i] += now_l * gain_l * stack_l;
                mix_r[i] += now_r * gain_r * stack_r;
            } else {
                v.flush_non_finite();
                return;
            }
            v.age += 1;
            if v.env.phase == Phase::Idle && (!c.filter_env_holds || silent(&v.filter_env)) && v.heat.spent(&c.heat, &curves.heat, i) {
                v.end(block_start + i as i64 + 1);
                return;
            }
        }
    }
}

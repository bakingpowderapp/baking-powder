use crate::params::{GlideCurve, Modulator, Quantize};
use crate::tv::{self, Shown, Tv};
use crate::{CombRead, FilterMode, FilterShape, Osc, Params, Shape, Slope, Target, Trigger, Wave, params};
use sdk::division::Division;
use sdk::dsp::noise::NoiseColor;
use sdk::draw::{DrawList, Rect};
use sdk::face::{self, Cell, Face, Faced, Format::*, Kind::*, Menu, READOUT_ROWS, View, choice, choose, desc, field, flag, flat, inert, menu, named, never, page, pick, refuses, switch, tempo, title, toggle, unless, value, viewed, wide};
use sdk::keyboard::Key;

const FILTER_MODES: [&str; 7] = ["SVF LP", "SVF BP", "SVF HP", "Ladder", "Ladder BP", "Comb", "Comb N"];
const FILTER_CHOICES: [(FilterMode, Option<FilterShape>); 7] = [
    (FilterMode::Svf, Some(FilterShape::LowPass)),
    (FilterMode::Svf, Some(FilterShape::BandPass)),
    (FilterMode::Svf, Some(FilterShape::HighPass)),
    (FilterMode::Ladder, Some(FilterShape::LowPass)),
    (FilterMode::Ladder, Some(FilterShape::BandPass)),
    (FilterMode::Comb, None),
    (FilterMode::CombNotch, None),
];
const COMB_READS: [&str; 2] = ["LIN", "AP"];
const LFO_MODES: [&str; 3] = ["TRIG", "RUN", "ONCE"];
const OVERSAMPLES: [&str; 3] = ["OVS 1×", "OVS 2×", "OVS 4×"];
const AMOUNT_DESC: &str = "100% moves pitch 12 semitones, cutoff 5 octaves, PW 45%, LFO rate 10 octaves, others full range.";
const FREE_DESC: &str = "Runs between notes; each note joins the current phase.";
const ALTERNATE_DESC: &str = "Alternates stacked copies hard left and right.";
const PHASE_DESC: &str = "Oscillator start phase at each note.";
const KEY_FOLLOW_DESC: &str = "Decay and release follow the key: 100% halves them each octave above C4, doubles below; attack stays.";
const A: usize = 0;
const B: usize = 1;
const C: usize = 2;
const WAVE_DESC: &str = "Wave level; click the picked cell again to choose its shape.";

fn noise_off(p: &Params) -> bool {
    !p.noise.on
}

fn no_sole_square(p: &Params) -> bool {
    !p.oscs().iter().any(Osc::pure)
}

fn offered(shapes: &[Wave; params::WAVES], slot: usize, option: usize) -> bool {
    let wave = Wave::ALL[option];
    shapes[slot] == wave || !shapes.contains(&wave)
}

fn modulator_index(p: &Params) -> usize {
    if p.modulator == Modulator::Osc3 { 2 } else { 1 }
}

fn bent_by_fm(p: &Params, osc: usize) -> bool {
    osc != modulator_index(p) && (p.oscs()[osc].fm > 0.0 || p.motion.moves(Target::fm_of(osc)))
}

fn stacks_in_step(p: &Params) -> bool {
    p.oscs().iter().all(|osc| osc.free || osc.unison <= 1)
}

fn filter_off(p: &Params) -> bool {
    !p.filter.on
}

fn slope_dead(p: &Params) -> bool {
    !combless(p) || (p.filter.mode == FilterMode::Svf && p.filter.shape == FilterShape::BandPass)
}

fn combless(p: &Params) -> bool {
    !matches!(p.filter.mode, FilterMode::Comb | FilterMode::CombNotch)
}

fn drive_dead(p: &Params) -> bool {
    filter_off(p) || !combless(p)
}

fn filter_velocity_dead(p: &Params) -> bool {
    !(p.filter.on && p.filter.env_amount != 0.0)
}

fn poly(p: &Params) -> bool {
    p.voices != 1
}

fn mono(p: &Params) -> bool {
    p.voices == 1
}

fn unstacked(p: &Params) -> bool {
    mono(p) || p.stack <= 1
}

fn unsampled(p: &Params) -> bool {
    !p.modulation.sampled
}

fn untuned(p: &Params) -> bool {
    p.modulation.tune2 == 0.0 && p.modulation.tune3 == 0.0 && p.modulation.tune4 == 0.0
}

fn filter_mode(p: &Params) -> f64 {
    let exact = FILTER_CHOICES.iter().position(|(mode, shape)| *mode == p.filter.mode && shape.is_none_or(|shape| shape == p.filter.shape));
    exact.or_else(|| FILTER_CHOICES.iter().position(|(mode, _)| *mode == p.filter.mode)).unwrap_or(0) as f64
}

fn set_filter_mode(p: &mut Params, v: f64) {
    let (mode, shape) = choice(&FILTER_CHOICES, v);
    p.filter.mode = mode;
    if let Some(shape) = shape {
        p.filter.shape = shape;
    }
}

fn filter_oversample(p: &Params) -> f64 {
    params::Filter::OVERSAMPLES.iter().position(|known| *known == p.filter.oversample).unwrap_or(0) as f64
}

fn set_filter_oversample(p: &mut Params, v: f64) {
    p.filter.oversample = choice(&params::Filter::OVERSAMPLES, v);
}

macro_rules! osc_row {
    ($row:expr, $osc:ident, $k:expr, $band:literal, $params:expr, $name:literal) => {
        [
            desc(value(Rise, $row, 0, "LVL", $params.level, Percent), "Oscillator output level."),
            desc(menu(named(value(Rise, $row, 1, "", $params.wave1, Percent), |p| p.$osc.shapes[0].short()), Menu::of(choose!(Wave::ALL, $osc.shapes[0]), Wave::LABELS, |p, option| offered(&p.$osc.shapes, 0, option))), WAVE_DESC),
            desc(menu(named(value(Rise, $row, 2, "", $params.wave2, Percent), |p| p.$osc.shapes[1].short()), Menu::of(choose!(Wave::ALL, $osc.shapes[1]), Wave::LABELS, |p, option| offered(&p.$osc.shapes, 1, option))), WAVE_DESC),
            desc(menu(named(value(Rise, $row, 3, "", $params.wave3, Percent), |p| p.$osc.shapes[2].short()), Menu::of(choose!(Wave::ALL, $osc.shapes[2]), Wave::LABELS, |p, option| offered(&p.$osc.shapes, 2, option))), WAVE_DESC),
            desc(menu(named(value(Rise, $row, 4, "", $params.wave4, Percent), |p| p.$osc.shapes[3].short()), Menu::of(choose!(Wave::ALL, $osc.shapes[3]), Wave::LABELS, |p, option| offered(&p.$osc.shapes, 3, option))), WAVE_DESC),
            desc(value(Centre, $row, 5, "TUN", $params.tune, Semitones), "Pitch offset in semitones."),
            desc(value(Centre, $row, 6, "FIN", $params.fine, SignedCents), "Fine tuning in cents."),
            desc(value(Rise, $row, 7, "UNI", $params.unison, Integer), "Unison copies per note."),
            desc(value(Rise, $row, 8, "DET", $params.detune, Cents), "Unison detune; outer copies reach +/-400 cents."),
            desc(value(Rise, $row, 9, "WID", $params.width, Percent), "Unison stereo width."),
            desc(value(Rise, $row, 10, "BLD", $params.blend, Percent), "Level of the detuned copies beside the centre copy; 100% equal, 0 the centre alone."),
            desc(unless(value(Rise, $row, 11, "PW", $params.pulse_width, Percent), |p| !p.$osc.pure()), "Pulse width; available only for square alone."),
            desc(inert(value(Rise, $row, 12, "FM", $params.fm, Percent), |p| modulator_index(p) == $k), "Phase modulation depth; LFM switches to frequency modulation."),
            desc(inert(toggle($row, 13, "SYN", $band, "HARD SYNC", flag!($osc.sync)), |p| modulator_index(p) == $k), "Resets phase on each modulator cycle."),
            desc(inert(toggle($row, 14, "BRT", $band, "BRIGHT", flag!($osc.bright)), |p| bent_by_fm(p, $k) && !p.$osc.linear_fm), concat!("Saw and square keep their top octave, unless FM bends ", $name)),
            desc(toggle($row, 15, "FRE", $band, "FREE RUNNING", flag!($osc.free)), FREE_DESC),
        ]
    };
}

macro_rules! env_cells {
    ($row:expr, $column:expr, $params:expr, $velocity_gate:expr) => {
        [
            desc(value(Rise, $row, $column, "DLY", $params.delay, GlideTime), "Delay before the envelope starts."),
            desc(value(Rise, $row, $column + 1, "ATK", $params.attack, Millis), "Attack time."),
            desc(value(Rise, $row, $column + 2, "DEC", $params.decay, Millis), "Decay time to sustain."),
            desc(value(Rise, $row, $column + 3, "SUS", $params.sustain, Percent), "Sustain level."),
            desc(value(Rise, $row, $column + 4, "REL", $params.release, Millis), "Release time."),
            desc(unless(value(Rise, $row, $column + 5, "VEL", $params.velocity, Percent), $velocity_gate), "Velocity sensitivity."),
            desc(value(Centre, $row, $column + 6, "KEY", $params.key_follow, SignedPercent), KEY_FOLLOW_DESC),
        ]
    };
}

macro_rules! lfo_row {
    ($row:expr, $lfo:ident, $params:expr, $band:literal) => {
        [
            desc(pick($row, 0, 3, $band, "TARGET", choose!(Target::ALL, motion.$lfo.target), Target::LABELS), "Per-voice LFO target; its knob sets the center."),
            desc(inert(value(Centre, $row, 3, "AMT", $params.amount, SignedPercent), |p| !p.motion.$lfo.aimed()), AMOUNT_DESC),
            desc(inert(pick($row, 4, 2, $band, "SHAPE", choose!(Shape::ALL, motion.$lfo.shape), Shape::LABELS), |p| !p.motion.$lfo.aimed()), "Random holds each cycle; Walk glides; Chaos wanders; Drawn follows the picture."),
            desc(tempo(inert(switch($row, 6, &["FREE", "SYNC"], $band, "SYNC", flag!(motion.$lfo.sync)), |p| !p.motion.$lfo.aimed()), |p, bpm| p.motion.$lfo.tempo(bpm)), "SYNC uses note divisions; FREE uses Hz."),
            desc(inert(refuses(pick($row, 7, 2, $band, "DIVISION", choose!(&Division::ALL, motion.$lfo.division), params::DIVISIONS), |p| !p.motion.$lfo.sync), |p| !(p.motion.$lfo.aimed() && p.motion.$lfo.sync)), "Synced cycle length."),
            desc(inert(refuses(value(Rise, $row, 9, "HZ", $params.rate, LfoHertz), |p| p.motion.$lfo.sync), |p| !p.motion.$lfo.aimed() || p.motion.$lfo.sync), "Free rate in Hz; RATE modulation works in either mode."),
            desc(inert(value(Rise, $row, 10, "PHS", $params.phase, Degrees), |p| !p.motion.$lfo.aimed()), "Start phase at each note."),
            desc(inert(switch($row, 11, &LFO_MODES, $band, "MODE", choose!(Trigger::ALL, motion.$lfo.trigger)), |p| !p.motion.$lfo.aimed()), "TRIG restarts per note; RUN shares one wave; ONCE plays then holds."),
            desc(inert(value(Rise, $row, 12, "KEY", $params.keytrack, Percent), |p| !p.motion.$lfo.aimed()), "Rate follows the key, FREE or SYNC: 100% doubles it each octave above C4, halves below."),
        ]
    };
}

macro_rules! motion_env_row {
    ($row:expr, $env:ident, $params:expr, $amount:expr, $band:literal) => {
        [
            desc(pick($row, 0, 3, $band, "TARGET", choose!(Target::ALL, motion.$env.target), Target::LABELS), "Per-voice envelope target; its knob sets rest, AMT sets peak."),
            desc(inert(value(Centre, $row, 3, "AMT", $amount, SignedPercent), |p| !p.motion.$env.aimed()), AMOUNT_DESC),
            desc(inert(value(Rise, $row, 4, "DLY", $params.delay, GlideTime), |p| !p.motion.$env.aimed()), "Delay before the envelope starts."),
            desc(inert(value(Rise, $row, 5, "ATK", $params.attack, Millis), |p| !p.motion.$env.aimed()), "Attack time."),
            desc(inert(value(Rise, $row, 6, "DEC", $params.decay, Millis), |p| !p.motion.$env.aimed()), "Decay time to sustain."),
            desc(inert(value(Rise, $row, 7, "SUS", $params.sustain, Percent), |p| !p.motion.$env.aimed()), "Sustain level."),
            desc(inert(value(Rise, $row, 8, "REL", $params.release, Millis), |p| !p.motion.$env.aimed()), "Release time."),
            desc(inert(value(Rise, $row, 9, "VEL", $params.velocity, Percent), |p| !p.motion.$env.aimed()), "Velocity sensitivity."),
            desc(inert(value(Centre, $row, 10, "KEY", $params.key_follow, SignedPercent), |p| !p.motion.$env.aimed()), KEY_FOLLOW_DESC),
        ]
    };
}

const TABS: [Cell<Params>; 12] = [
    desc(page(0, 0, "A", "CORE", "VIEW A", A), "Oscillators, modulator, amp/filter envelopes and filter."),
    desc(page(0, 1, "B", "CORE", "VIEW B", B), "Per-voice LFOs/envelopes, MOD, noise, oscillator phases, carry and step shares."),
    desc(page(0, 2, "C", "CORE", "VIEW C", C), "Drift, scatter and heat for each voice."),
    desc(value(Rise, 0, 3, "VOI", params::VOICES, Integer), "Voice count; 1 is mono."),
    desc(toggle(0, 4, "FRSH", "VOICE", "FRESH VOICE", flag!(fresh)), "Repeated notes use a free voice; mono crossfades over 4 ms."),
    desc(value(Rise, 0, 5, "GLD", params::GLIDE, GlideTime), "Glide time; poly voices glide from the last played note."),
    desc(toggle(0, 6, "LEG", "MONO", "LEGATO", flag!(mono.legato)), "Glide only between overlapping notes."),
    desc(unless(toggle(0, 7, "RTG", "MONO", "RETRIGGER", flag!(mono.retrigger)), poly), "Retrigger envelopes on each note."),
    desc(switch(0, 8, GlideCurve::LABELS, "MONO", "GLIDE CURVE", choose!(GlideCurve::ALL, mono.curve)), "EASE settles exponentially; LIN takes GLD per slide, OCT per octave."),
    desc(unless(value(Rise, 0, 9, "STK", params::STACK, Integer), mono), "Voices each note plays, each with its own phases and noise, counted against VOI."),
    desc(inert(value(Rise, 0, 10, "STC", params::STACK_CENTS, CentsTenth), unstacked), "Cents the stack's voices spread either side of the note."),
    desc(inert(value(Rise, 0, 11, "STW", params::STACK_SPREAD, Percent), unstacked), "Stack voices alternately left and right."),
];

const OSC1: [Cell<Params>; 16] = viewed(osc_row!(1, osc1, 0, "OSC 1", params::OSC1, "osc 1"), A);
const OSC2: [Cell<Params>; 16] = viewed(osc_row!(2, osc2, 1, "OSC 2", params::OSC2, "osc 2"), A);
const OSC3: [Cell<Params>; 16] = viewed(osc_row!(3, osc3, 2, "OSC 3", params::OSC3, "osc 3"), A);
const OSC4: [Cell<Params>; 16] = viewed(osc_row!(4, osc4, 3, "OSC 4", params::OSC4, "osc 4"), A);
static TOGGLES: [Cell<Params>; 14] = viewed(
    [
        desc(toggle(5, 0, "ALT1", "OSC 1", "ALTERNATE SIDES", flag!(osc1.alternate)), ALTERNATE_DESC),
        desc(toggle(5, 1, "INV1", "OSC 1", "INVERT", flag!(osc1.invert)), "Inverts oscillator polarity and its modulation output."),
        desc(toggle(5, 2, "ALT2", "OSC 2", "ALTERNATE SIDES", flag!(osc2.alternate)), ALTERNATE_DESC),
        desc(toggle(5, 3, "INV2", "OSC 2", "INVERT", flag!(osc2.invert)), "Inverts oscillator polarity and its modulation output."),
        desc(toggle(5, 4, "ALT3", "OSC 3", "ALTERNATE SIDES", flag!(osc3.alternate)), ALTERNATE_DESC),
        desc(toggle(5, 5, "INV3", "OSC 3", "INVERT", flag!(osc3.invert)), "Inverts oscillator polarity and its modulation output."),
        desc(toggle(5, 6, "ALT4", "OSC 4", "ALTERNATE SIDES", flag!(osc4.alternate)), ALTERNATE_DESC),
        desc(toggle(5, 7, "INV4", "OSC 4", "INVERT", flag!(osc4.invert)), "Inverts oscillator polarity."),
        desc(inert(toggle(5, 8, "LFM1", "OSC 1", "LINEAR FM", flag!(osc1.linear_fm)), |p| !bent_by_fm(p, 0)), "Linear FM; at 100%, deviation reaches +/-100%."),
        desc(inert(toggle(5, 9, "LFM2", "OSC 2", "LINEAR FM", flag!(osc2.linear_fm)), |p| !bent_by_fm(p, 1)), "Linear FM; at 100%, deviation reaches +/-100%."),
        desc(inert(toggle(5, 10, "LFM3", "OSC 3", "LINEAR FM", flag!(osc3.linear_fm)), |p| !bent_by_fm(p, 2)), "Linear FM; at 100%, deviation reaches +/-100%."),
        desc(inert(toggle(5, 11, "LFM4", "OSC 4", "LINEAR FM", flag!(osc4.linear_fm)), |p| !bent_by_fm(p, 3)), "Linear FM; at 100%, deviation reaches +/-100%."),
        desc(switch(5, 12, Modulator::LABELS, "OSC 1", "MODULATOR", choose!(Modulator::ALL, modulator)), "OSC2 or OSC3 modulates the others; its own FM and SYN are disabled."),
        desc(inert(value(Rise, 5, 13, "RND", params::PHASE_RANDOM, Percent), stacks_in_step), "Unison start-phase scatter around PHS: 0 aligned, 100% random."),
    ],
    A,
);
const AMP: [Cell<Params>; 7] = viewed(env_cells!(6, 0, params::AMP, never), A);
const FILTER_ENV: [Cell<Params>; 7] = viewed(env_cells!(6, 7, params::FILTER_ENV, filter_velocity_dead), A);
static FILTER: [Cell<Params>; 15] = viewed(
    [
        desc(inert(toggle(6, 14, "KEEP", "FILTER", "ENV HOLDS VOICE", flag!(filter.env_holds)), filter_off), "Keep the voice alive until both amp and filter envelopes release."),
        desc(inert(value(Centre, 6, 15, "RVEL", params::RESONANCE_VELOCITY, SignedPercent), filter_off), "Velocity adds resonance; 100% spans its range, negative subtracts."),
        desc(toggle(7, 0, "FLT", "FILTER", "FILTER ON", flag!(filter.on)), "Enable the filter."),
        desc(pick(7, 1, 3, "FILTER", "MODE", field!(filter_mode, set_filter_mode), &FILTER_MODES), "Filter model and shape."),
        desc(unless(switch(7, 4, &Slope::LABELS, "FILTER", "SLOPE", choose!(&Slope::ALL, filter.slope)), slope_dead), "Slope in dB/octave. Ladder BP supports 12/24; 18 selects 24."),
        desc(inert(value(Rise, 7, 5, "CUT", params::CUTOFF, Hertz), filter_off), "Filter cutoff frequency."),
        desc(inert(value(Rise, 7, 6, "RES", params::RESONANCE, Percent), filter_off), "Filter resonance."),
        desc(inert(value(Centre, 7, 7, "ENV", params::ENV_AMOUNT, SignedPercent), filter_off), "Filter-envelope cutoff depth."),
        desc(inert(value(Rise, 7, 8, "KEY", params::KEYTRACK, Percent), filter_off), "Cutoff key tracking."),
        desc(inert(value(Rise, 7, 9, "DRV", params::DRIVE, Percent), drive_dead), "Filter saturation drive."),
        desc(inert(value(Rise, 7, 10, "FBK", params::FEEDBACK, Percent), drive_dead), "Positive feedback before DRV; raises passband gain, flattens the knee."),
        desc(inert(value(Rise, 7, 11, "SLW", params::MOD_SLEW, GlideTime), filter_off), "Slews cutoff modulation from envelopes, view B and MOD; 0 is instant."),
        desc(inert(value(Centre, 7, 12, "FFM", params::FILTER_FM, SignedPercent), filter_off), "Audio-rate cutoff modulation: 100% spans five octaves; negative inverts."),
        desc(inert(pick(7, 13, 2, "FILTER", "OVERSAMPLE", field!(filter_oversample, set_filter_oversample), &OVERSAMPLES), |p| !combless(p)), "Extra 1×/2×/4× for SVF/ladder; cutoff ceiling 0.45/0.9/1.8× sample rate."),
        desc(unless(pick(7, 15, 1, "FILTER", "COMB READ", choose!(CombRead::ALL, filter.comb_read), &COMB_READS), combless), "LIN damps upper harmonics; AP keeps their level."),
    ],
    A,
);

const LFO1_ROW: [Cell<Params>; 9] = viewed(lfo_row!(1, lfo1, params::LFO1, "LFO 1"), B);
const LFO2_ROW: [Cell<Params>; 9] = viewed(lfo_row!(2, lfo2, params::LFO2, "LFO 2"), B);
const ENV1_ROW: [Cell<Params>; 9] = viewed(motion_env_row!(3, env1, params::MOTION_ENV1, params::MOTION_AMOUNTS.env1, "ENV 1"), B);
const ENV2_ROW: [Cell<Params>; 9] = viewed(motion_env_row!(4, env2, params::MOTION_ENV2, params::MOTION_AMOUNTS.env2, "ENV 2"), B);
static MOD_ROW: [Cell<Params>; 12] = viewed(
    [
        desc(value(Centre, 5, 0, "LV1", params::MOD_LEVEL1, SignedPercent), "MOD depth for osc 1 level; 100% spans its full range."),
        desc(value(Centre, 5, 1, "LV2", params::MOD_LEVEL2, SignedPercent), "MOD depth for osc 2 level; 100% spans its full range."),
        desc(value(Centre, 5, 2, "TUN", params::MOD_TUNE2, Semitones), "Osc 2 pitch offset at full MOD, in semitones."),
        desc(value(Centre, 5, 3, "TUN3", params::MOD_TUNE3, Semitones), "Osc 3 pitch offset at full MOD, in semitones."),
        desc(value(Centre, 5, 4, "TUN4", params::MOD_TUNE4, Semitones), "Osc 4 pitch offset at full MOD, in semitones."),
        desc(unless(pick(5, 5, 1, "MOD", "QUANTIZE", choose!(Quantize::ALL, modulation.quantize), Quantize::LABELS), untuned), "Snaps TUN/TUN3/TUN4 to the chosen intervals, chord or scale, before LAG."),
        desc(value(Rise, 5, 6, "LAG", params::MOD_LAG, GlideTime), "Glide time from the previous note's MOD value."),
        desc(toggle(5, 7, "SMP", "MOD", "SAMPLED", flag!(modulation.sampled)), "Each note holds VAL sampled at onset, scaled by its step's MOD."),
        desc(inert(value(Centre, 5, 8, "VAL", params::MOD_SAMPLE, SignedPercent), unsampled), "Value SMP samples at each note start; can be automated."),
        desc(wide(inert(value(Centre, 5, 9, "CUTOFF", params::MOD_CUTOFF, Octaves), filter_off), 2), "Cutoff offset at full MOD, in octaves."),
        desc(wide(value(Centre, 5, 11, "PAN", params::MOD_PAN, SignedPercent), 2), "Pan offset at full MOD; 100% is hard right."),
        desc(wide(unless(value(Centre, 5, 13, "PULSE WIDTH", params::MOD_PULSE_WIDTH, SignedPercent), no_sole_square), 3), "Pulse width offset at full MOD; available only for square alone."),
    ],
    B,
);
static NOISE: [Cell<Params>; 7] = viewed(
    [
        desc(toggle(6, 0, "NSE", "NOISE", "NOISE ON", flag!(noise.on)), "Per-voice noise through the filter and amp envelope."),
        desc(pick(6, 1, 2, "NOISE", "COLOR", choose!(NoiseColor::ALL, noise.color), NoiseColor::LABELS), "Noise color."),
        desc(inert(value(Rise, 6, 3, "LVL", params::LEVEL, Percent), noise_off), "Noise level."),
        desc(inert(value(Rise, 6, 4, "WID", params::WIDTH, Percent), noise_off), "Noise stereo width."),
        desc(inert(value(Centre, 6, 5, "TON", params::TONE, Shelf), noise_off), "Noise low-pass/high-pass tone."),
        desc(inert(value(Rise, 6, 6, "HP", params::NOISE_HIGH_PASS, HertzOffAt20), noise_off), "Noise high-pass before the main filter."),
        desc(inert(value(Rise, 6, 7, "LP", params::NOISE_LOW_PASS, HertzOffAt20k), noise_off), "Noise low-pass before the main filter."),
    ],
    B,
);
static PHASES: [Cell<Params>; 12] = viewed(
    [
        desc(inert(value(Rise, 7, 0, "PHS1", params::OSC1.phase, Degrees), |p| p.osc1.free), PHASE_DESC),
        desc(inert(value(Rise, 7, 1, "PHS2", params::OSC2.phase, Degrees), |p| p.osc2.free), PHASE_DESC),
        desc(inert(value(Rise, 7, 2, "PHS3", params::OSC3.phase, Degrees), |p| p.osc3.free), PHASE_DESC),
        desc(inert(value(Rise, 7, 3, "PHS4", params::OSC4.phase, Degrees), |p| p.osc4.free), PHASE_DESC),
        desc(toggle(7, 4, "CRY1", "OSC 1", "CARRY PHASE", flag!(osc1.carry)), "Continue phase through voice silence at the last pitch."),
        desc(toggle(7, 5, "CRY2", "OSC 2", "CARRY PHASE", flag!(osc2.carry)), "Continue phase through voice silence at the last pitch."),
        desc(toggle(7, 6, "CRY3", "OSC 3", "CARRY PHASE", flag!(osc3.carry)), "Continue phase through voice silence at the last pitch."),
        desc(toggle(7, 7, "CRY4", "OSC 4", "CARRY PHASE", flag!(osc4.carry)), "Continue phase through voice silence at the last pitch."),
        desc(inert(value(Rise, 7, 8, "SYP1", params::OSC1.sync_phase, Degrees), |p| !p.osc1.sync), "Hard-sync reset phase: 0° is the top; negative values restart below it."),
        desc(inert(value(Rise, 7, 9, "SYP2", params::OSC2.sync_phase, Degrees), |p| !p.osc2.sync), "Hard-sync reset phase: 0° is the top; negative values restart below it."),
        desc(inert(value(Rise, 7, 10, "SYP3", params::OSC3.sync_phase, Degrees), |p| !p.osc3.sync), "Hard-sync reset phase: 0° is the top; negative values restart below it."),
        desc(inert(value(Rise, 7, 11, "SYP4", params::OSC4.sync_phase, Degrees), |p| !p.osc4.sync), "Hard-sync reset phase: 0° is the top; negative values restart below it."),
    ],
    B,
);

static STEPS: [Cell<Params>; 4] = viewed(
    [
        desc(value(Centre, 7, 12, "STP1", params::OSC1.step_share, SignedPercent), "Share of an arp step's semitones and octave shift osc 1 follows: 100% whole, 0 none, staying on the held key, -100% turned over"),
        desc(value(Centre, 7, 13, "STP2", params::OSC2.step_share, SignedPercent), "Share of an arp step's semitones and octave shift osc 2 follows: 100% whole, 0 none, staying on the held key, -100% turned over"),
        desc(value(Centre, 7, 14, "STP3", params::OSC3.step_share, SignedPercent), "Share of an arp step's semitones and octave shift osc 3 follows: 100% whole, 0 none, staying on the held key, -100% turned over"),
        desc(value(Centre, 7, 15, "STP4", params::OSC4.step_share, SignedPercent), "Share of an arp step's semitones and octave shift osc 4 follows: 100% whole, 0 none, staying on the held key, -100% turned over"),
    ],
    B,
);

static DRIFT_ROW: [Cell<Params>; 6] = viewed(
    [
        title(1, 0, "DRIFT"),
        desc(wide(value(Rise, 2, 0, "AMOUNT", params::DRIFT, CentsTenth), 3), "Slow random pitch wander in cents: each voice, oscillator and unison copy on its own path, running on across notes."),
        desc(wide(value(Centre, 2, 3, "RATE", params::DRIFT_RATE, Times), 3), "Speed of all drift; 1 is the usual pace."),
        desc(wide(value(Rise, 2, 6, "JITTER", params::JITTER, CentsTenth), 3), "Fast random pitch flutter in cents, each oscillator copy on its own."),
        desc(wide(inert(value(Rise, 2, 9, "CUTOFF", params::CUTOFF_DRIFT, Cents), filter_off), 3), "Slow random cutoff wander per voice, in cents."),
        desc(wide(unless(value(Rise, 2, 12, "PULSE WIDTH", params::WIDTH_DRIFT, Percent), no_sole_square), 3), "Slow random pulse width wander per oscillator; available only for square alone."),
    ],
    C,
);
static SCATTER_ROW: [Cell<Params>; 7] = viewed(
    [
        title(3, 0, "SCATTER"),
        desc(wide(value(Rise, 4, 0, "TUNE", params::DETUNE, CentsTenth), 3), "Independent random oscillator detune per note, in cents."),
        desc(wide(inert(value(Rise, 4, 3, "CUTOFF", params::CUTOFF_SCATTER, Cents), filter_off), 3), "Random cutoff offset per note, in cents."),
        desc(wide(value(Rise, 4, 6, "ENVELOPE", params::ENV_SCATTER, Percent), 3), "Random envelope speed per note; 100% spans half to double."),
        desc(wide(unless(value(Rise, 4, 9, "PULSE WIDTH", params::WIDTH_SCATTER, Percent), no_sole_square), 3), "Random pulse width offset per note; available only for square alone."),
        desc(wide(value(Rise, 4, 12, "LEVEL", params::LEVEL_SCATTER, DecibelsUnsigned), 2), "Random level offset per note, in dB."),
        desc(wide(value(Rise, 4, 14, "PAN", params::PAN_SCATTER, Percent), 2), "Random pan position per note."),
    ],
    C,
);
static HEAT_ROW: [Cell<Params>; 5] = viewed(
    [
        title(5, 0, "HEAT"),
        desc(wide(value(Rise, 6, 0, "PRE", params::HEAT_PRE, Percent), 3), "Soft saturation before the filter, per voice."),
        desc(wide(value(Rise, 6, 3, "POST", params::HEAT_POST, Percent), 3), "Soft saturation after the amp, per voice."),
        desc(wide(value(Rise, 6, 6, "LEAN", params::HEAT_LEAN, Percent), 3), "Tilts the saturation to one side."),
        desc(wide(value(Rise, 6, 9, "LOW CUT", params::AC, HertzOff), 3), "Low cut per voice in Hz; off at zero."),
    ],
    C,
);
static BYPASS: [Cell<Params>; 1] = viewed([desc(wide(toggle(7, 14, "BYPASS", "ANALOG", "BYPASS", flag!(analog.bypass)), 2), "Switches all of view C off, to compare.")], C);

pub static CORE: [Cell<Params>; 209] = flat(&[&TABS, &OSC1, &OSC2, &OSC3, &OSC4, &TOGGLES, &AMP, &FILTER_ENV, &FILTER, &LFO1_ROW, &LFO2_ROW, &ENV1_ROW, &ENV2_ROW, &MOD_ROW, &NOISE, &PHASES, &STEPS, &DRIFT_ROW, &SCATTER_ROW, &HEAT_ROW, &BYPASS]);

impl Faced for Params {
    fn readout_rows(&self) -> i32 {
        READOUT_ROWS
    }

    fn hover(&self, face: &mut Face, point: Option<(i32, i32)>) -> bool {
        face.hover(point, READOUT_ROWS, &CORE)
    }

    fn down(&mut self, face: &mut Face, x: i32, y: i32, bpm: f64) -> bool {
        face.down(x, y, READOUT_ROWS, &CORE, self, bpm)
    }

    fn moved(&mut self, face: &mut Face, x: i32, y: i32) -> bool {
        face.moved(x, y, READOUT_ROWS, &CORE, self)
    }

    fn wheel(&mut self, face: &mut Face, x: i32, y: i32, dy: i32) -> bool {
        face.wheel(x, y, dy, READOUT_ROWS, &CORE, self)
    }

    fn up(&mut self, face: &mut Face, x: i32, y: i32) -> bool {
        face.up(x, y, READOUT_ROWS, &CORE, self)
    }

    fn begin(&self, face: &mut Face, first: Option<char>) -> bool {
        face.begin(&CORE, self, first)
    }

    fn key(&mut self, face: &mut Face, key: Key, command: bool, bpm: f64) -> bool {
        face.key(key, command, &CORE, self, bpm)
    }

    fn nudge(&mut self, face: &mut Face, direction: i32, coarse: bool, bpm: f64) -> bool {
        face.nudge(&CORE, self, direction, coarse, bpm)
    }

    fn index_at(&self, x: i32, y: i32, view: View) -> Option<usize> {
        face::index_at(&CORE, self, READOUT_ROWS, x, y, view)
    }

    fn setting_of(&self, cell: usize) -> Option<(&'static str, &'static str, f64)> {
        face::setting_at(&CORE, self, cell)
    }

    fn assign(&mut self, band: &str, param: &str, value: f64) -> Option<usize> {
        face::assign_named(&CORE, self, band, param, value)
    }

    fn restore(&mut self, cell: usize) -> Option<(&'static str, &'static str)> {
        let restored = face::restore_at(&CORE, self, cell)?;
        if let Some((which, stage)) = tv::stage_of(cell) {
            tv::straighten(self, which, stage);
        }
        Some(restored)
    }

    fn armable(&self, cell: usize) -> Option<(&'static str, &'static str)> {
        face::armable_cell(&CORE, cell)
    }

    fn armable_at(&self, x: i32, y: i32, view: View) -> Option<(&'static str, &'static str)> {
        face::armable_at(&CORE, self, READOUT_ROWS, x, y, view)
    }

    fn automatable_of(&self, band: &str, param: &str) -> Option<(usize, &'static str, &'static str)> {
        face::automatable_of(&CORE, band, param)
    }

    fn spelling(&self, band: &str, param: &str, travel: f64) -> Option<(String, &'static str)> {
        face::spelling(&CORE, band, param, travel)
    }

    fn automated_rect(&self, band: &str, param: &str, view: View) -> Option<Rect> {
        face::automated_rect(&CORE, READOUT_ROWS, band, param, view)
    }
}

impl Params {
    pub fn draw(&self, face: &Face, list: &mut DrawList, tv: Option<(&Tv, Shown)>, idle: impl FnOnce(&mut DrawList, Rect)) {
        match tv {
            Some((tv, shown)) => {
                let desc = face.shown_cell().or_else(|| face.picked()).map_or("", |index| CORE[index].desc);
                let preview = face.shown_value(&CORE, self).and_then(|(_, value)| Wave::LABELS.iter().position(|label| *label == value)).map(|option| Wave::ALL[option]);
                face.draw_pictured(list, READOUT_ROWS, &CORE, self, |list, inner| tv.draw(list, inner, self, shown, preview, desc));
            }
            None => face.draw(list, READOUT_ROWS, &CORE, self, idle),
        }
    }
}

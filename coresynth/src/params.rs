use crate::face::CORE;
use sdk::division::{self, Division};
use sdk::dsp::envelope::FALL;
use sdk::dsp::noise::NoiseColor;
use sdk::face;
use sdk::json::Json;
use sdk::param::{self, BIPOLAR, Knob, Param, Patch, Scale, UNIT, bank, field, flat, int, linear, log, skewed, table};
use sdk::schema::{Fields, choice, items, name, number, params};
use std::sync::OnceLock;

choice!(pub FilterMode { Svf = "svf", Ladder = "ladder", Comb = "comb", CombNotch = "comb-notch" });
choice!(pub FilterShape { LowPass = "low-pass", BandPass = "band-pass", HighPass = "high-pass" });
choice!(pub CombRead { Linear = "linear", Allpass = "allpass" });
choice!(pub Shape { Sine = "Sine" / "sine", Triangle = "Triangle" / "triangle", Saw = "Saw" / "saw", Ramp = "Ramp" / "ramp", Square = "Square" / "square", Random = "Random" / "random", Walk = "Walk" / "walk", Chaos = "Chaos" / "chaos", Drawn = "Drawn" / "drawn" });
choice!(pub Trigger { Trig = "trig", Run = "run", Once = "once" });
choice!(pub Wave { Saw = "SAW" / "saw", Square = "SQUARE" / "square", Triangle = "TRIANGLE" / "triangle", Sine = "SINE" / "sine", HalfSine = "HALF SINE" / "half-sine", FullSine = "FULL SINE" / "full-sine", SinePulse = "SINE PULSE" / "sine-pulse", Parabola = "PARABOLA" / "parabola", Trapezoid = "TRAPEZOID" / "trapezoid", DoubleSaw = "DOUBLE SAW" / "double-saw", Stair = "STAIR" / "stair", TriSaw = "TRI SAW" / "tri-saw", Soft = "SOFT SQUARE" / "soft-square", Organ = "ORGAN" / "organ", SineCubed = "SINE CUBED" / "sine-cubed", Drawn = "DRAWN" / "drawn" });

pub const WAVES: usize = 4;

impl Wave {
    pub const SHORT: [&'static str; 16] = ["SAW", "SQR", "TRI", "SIN", "HSN", "FSN", "SPL", "PAR", "TRP", "DSW", "STR", "TSW", "SFT", "ORG", "SN3", "DRW"];

    pub fn short(self) -> &'static str {
        Wave::SHORT[self.index()]
    }
}
choice!(pub Target { Off = "OFF" / "off", Pitch = "PITCH" / "pitch", Pitch1 = "PITCH 1" / "pitch1", Pitch2 = "PITCH 2" / "pitch2", Pitch3 = "PITCH 3" / "pitch3", Pitch4 = "PITCH 4" / "pitch4", Level1 = "LEVEL 1" / "level1", Level2 = "LEVEL 2" / "level2", Level3 = "LEVEL 3" / "level3", Level4 = "LEVEL 4" / "level4", PulseWidth1 = "PW 1" / "pulseWidth1", PulseWidth2 = "PW 2" / "pulseWidth2", PulseWidth3 = "PW 3" / "pulseWidth3", PulseWidth4 = "PW 4" / "pulseWidth4", Pan = "PAN" / "pan", Cutoff = "CUTOFF" / "cutoff", Fm = "FM" / "fm", Fm1 = "FM 1" / "fm1", Fm2 = "FM 2" / "fm2", Fm3 = "FM 3" / "fm3", Fm4 = "FM 4" / "fm4", Noise = "NOISE" / "noise", Amp = "AMP" / "amp", Resonance = "RESONANCE" / "resonance", Lfo1Rate = "LFO1 RATE" / "lfo1Rate", Lfo2Rate = "LFO2 RATE" / "lfo2Rate", Lfo1Amount = "LFO1 AMT" / "lfo1Amount", Lfo2Amount = "LFO2 AMT" / "lfo2Amount" });
choice!(pub Modulator { Osc2 = "OSC2" / "osc2", Osc3 = "OSC3" / "osc3" });
choice!(pub Quantize { Off = "Off" / "off", Chromatic = "Semi" / "chromatic", Octaves = "Oct" / "octaves", Fifths = "5th" / "fifths", MajorChord = "Maj" / "major-chord", MinorChord = "Min" / "minor-chord", MajorScale = "MajS" / "major-scale", MinorScale = "MinS" / "minor-scale" });

impl Quantize {
    fn steps(self) -> &'static [f64] {
        match self {
            Quantize::Off => &[],
            Quantize::Chromatic => &[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0],
            Quantize::Octaves => &[0.0],
            Quantize::Fifths => &[0.0, 7.0],
            Quantize::MajorChord => &[0.0, 4.0, 7.0],
            Quantize::MinorChord => &[0.0, 3.0, 7.0],
            Quantize::MajorScale => &[0.0, 2.0, 4.0, 5.0, 7.0, 9.0, 11.0],
            Quantize::MinorScale => &[0.0, 2.0, 3.0, 5.0, 7.0, 8.0, 10.0],
        }
    }

    pub fn snap(self, semitones: f64) -> f64 {
        let steps = self.steps();
        if steps.is_empty() {
            return semitones;
        }
        let rounded = semitones.round();
        let class = rounded.rem_euclid(12.0);
        if steps.contains(&class) {
            return rounded;
        }
        let base = rounded - class;
        if class == 11.0 {
            return base + 12.0;
        }
        base + steps.iter().rev().find(|step| **step < class).copied().unwrap_or(0.0)
    }
}

choice!(pub GlideCurve { Ease = "EASE" / "ease", Linear = "LIN" / "linear", Octave = "OCT" / "octave" });
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Slope {
    Db12,
    Db18,
    #[default]
    Db24,
}

impl Slope {
    pub const ALL: [Slope; 3] = [Slope::Db12, Slope::Db18, Slope::Db24];
    pub const LABELS: [&str; 3] = ["12", "18", "24"];

    pub const fn db(self) -> f64 {
        match self {
            Slope::Db12 => 12.0,
            Slope::Db18 => 18.0,
            Slope::Db24 => 24.0,
        }
    }

    fn nearest(db: f64) -> Slope {
        if db < 15.0 {
            Slope::Db12
        } else if db < 21.0 {
            Slope::Db18
        } else {
            Slope::Db24
        }
    }
}

params!(Osc {
    custom shapes: [Wave; WAVES] = Osc::SHAPES, "shapes", "4 names, one per wave slot, default saw, square, triangle, sine; any of saw, square, triangle, sine, half-sine, full-sine, sine-pulse, parabola, trapezoid, double-saw, stair, tri-saw, soft-square, organ, sine-cubed, drawn; drawn plays points", |p, d, f| if p.shapes != d.shapes { f.push("shapes", Json::Array(p.shapes.iter().map(|wave| Json::from(wave.name())).collect())) }, |json, d| {
        let mut shapes = d.shapes;
        for (slot, item) in shapes.iter_mut().zip(items(json, "shapes")) {
            if let Some(wave) = item.as_str().and_then(Wave::from_name) {
                *slot = wave;
            }
        }
        shapes
    };
    custom levels: [f64; WAVES] = Osc::LEVELS, "levels", "4 numbers 0 to 1, each wave slot's level, default 1, 0, 0, 0", |p, d, f| if p.levels != d.levels { f.push("levels", Json::Array(p.levels.iter().map(|level| Json::Number(*level)).collect())) }, |json, d| {
        let mut levels = d.levels;
        for (slot, item) in levels.iter_mut().zip(items(json, "levels")) {
            *slot = item.as_f64().unwrap_or(*slot).clamp(0.0, 1.0);
        }
        levels
    };
    custom points: face::Drawn = face::Drawn::RAMP, "points", face::Drawn::manual("2 to 32 points in x order, one cycle of the drawn wave, heard in the slot shaped drawn; y 0 the bottom, 1 the top; ends pinned to x 0 and 1; default a ramp 0 to 1"), |p, d, f| if p.shapes.contains(&Wave::Drawn) || p.points != d.points { f.push("points", p.points.to_json()) }, |json, d| face::Drawn::from_json(items(json, "points")).unwrap_or(d.points);
    number level = 1.0, "level", 0.0..=1.0;
    custom tune: f64 = 0.0, "tuneSemitones", "whole number -48 to 48, default 0", |p, d, f| f.number("tuneSemitones", p.tune, d.tune), |json, d| number(json, "tuneSemitones", d.tune).clamp(-48.0, 48.0).round();
    number fine = 0.0, "fineCents", -100.0..=100.0;
    number step_share = 1.0, "stepShare", -1.0..=1.0;
    whole unison: usize = 1, "unison", 1.0..=16.0;
    number detune = 0.0, "detuneCents", 0.0..=Osc::MAX_DETUNE_CENTS;
    number width = 1.0, "width", 0.0..=1.0;
    flag alternate = false, "unisonAlternate";
    number blend = 1.0, "unisonBlend", 0.0..=1.0;
    number fm = 0.0, "fmAmount", 0.0..=1.0;
    flag linear_fm = false, "linearFm";
    flag sync = false, "hardSync";
    number sync_phase = 0.0, "syncPhase", -0.5..=1.0;
    number pulse_width = 0.5, "pulseWidth", 0.05..=0.95;
    flag bright = false, "bright";
    flag free = false, "freeRunning";
    flag carry = false, "carryPhase";
    flag invert = false, "invert";
    number phase = 0.0, "phase", 0.0..=1.0;
});

impl Osc {
    pub const MAX_DETUNE_CENTS: f64 = 400.0;
    pub const SHAPES: [Wave; WAVES] = [Wave::Saw, Wave::Square, Wave::Triangle, Wave::Sine];
    pub const LEVELS: [f64; WAVES] = [1.0, 0.0, 0.0, 0.0];

    pub const fn with_level(level: f64) -> Osc {
        Osc { level, ..Osc::DEFAULT }
    }

    pub fn sounding(&self) -> bool {
        self.levels.iter().any(|level| *level > 0.0)
    }

    pub fn pure(&self) -> bool {
        let square = |slot: usize| self.shapes[slot] == Wave::Square;
        (0..WAVES).any(|slot| self.levels[slot] > 0.0 && square(slot)) && (0..WAVES).all(|slot| self.levels[slot] <= 0.0 || square(slot))
    }
}

params!(Envelope {
    number delay_ms = 0.0, "delayMs", 0.0..=Envelope::MAX_WAIT_MS;
    lead attack_ms = 5.0, "attackMs", Envelope::MIN_ATTACK_MS..=Envelope::MAX_MS;
    lead decay_ms = 200.0, "decayMs", Envelope::MIN_MS..=Envelope::MAX_MS;
    lead sustain = 0.8, "sustain", 0.0..=1.0;
    lead release_ms = 200.0, "releaseMs", Envelope::MIN_MS..=Envelope::MAX_MS;
    number velocity = 1.0, "velocityAmount", 0.0..=1.0;
    number key_follow = 0.0, "keyFollow", -1.0..=1.0;
    number attack_bend = 0.0, "attackBend", -1.0..=1.0;
    number decay_bend = FALL, "decayBend", -1.0..=1.0;
    number release_bend = FALL, "releaseBend", -1.0..=1.0;
});

impl Envelope {
    pub const FILTER: Envelope = Envelope { decay_ms: 300.0, sustain: 0.4, ..Envelope::DEFAULT };
    pub const MOTION: Envelope = Envelope { decay_ms: 300.0, sustain: 0.0, velocity: 0.0, ..Envelope::DEFAULT };
    pub const MIN_MS: f64 = 1.0;
    pub const MIN_ATTACK_MS: f64 = 0.25;
    pub const MAX_MS: f64 = 30_000.0;
    pub const MAX_WAIT_MS: f64 = 5000.0;
    pub const MIDDLE_C: f64 = 60.0;

    pub fn seconds(&self, midi: u8) -> [f32; 5] {
        let follow = if self.key_follow == 0.0 { 1.0 } else { (-self.key_follow * (f64::from(midi) - Envelope::MIDDLE_C) / 12.0).exp2() };
        [self.delay_ms, self.attack_ms, 0.0, self.decay_ms * follow, self.release_ms * follow].map(|ms| (ms / 1000.0) as f32)
    }

    pub fn bends(&self) -> [f64; 3] {
        [self.attack_bend, self.decay_bend, self.release_bend]
    }
}

params!(Filter {
    switch on = false, "on";
    custom mode: FilterMode = FilterMode::Svf, "mode", "one of svf-low-pass, svf-band-pass, svf-high-pass, ladder, ladder-band-pass, comb, comb-notch; default svf-low-pass; comb peaks at every multiple of cutoffHz, comb-notch halfway between, resonance sets how long they ring; both ignore drive, slopeDbPerOctave and oversample", |p, d, f| f.lead("mode", Json::String(filter_word(p.mode, p.shape)), filter_word(p.mode, p.shape) == filter_word(d.mode, d.shape)), |json, d| filter_choice(json).map_or(d.mode, |(mode, _)| mode);
    custom shape: FilterShape = FilterShape::LowPass, "mode", "", |_, _, _| {}, |json, d| filter_choice(json).map_or(d.shape, |(_, shape)| shape);
    custom slope: Slope = Slope::Db24, "slopeDbPerOctave", "12, 18 or 24, default 24", |p, d, f| f.lead("slopeDbPerOctave", Json::Number(p.slope.db()), p.slope == d.slope), |json, d| Slope::nearest(number(json, "slopeDbPerOctave", d.slope.db()));
    custom oversample: usize = 4, "oversample", "1, 2 or 4, default 4",|p, d, f| f.number("oversample", p.oversample as f64, d.oversample as f64), |json, d| oversample_of(number(json, "oversample", d.oversample as f64));
    choice comb_read: CombRead = CombRead::Linear, "combRead";
    lead cutoff_hz = 2000.0, "cutoffHz", Filter::MIN_CUTOFF_HZ..=Filter::MAX_HZ;
    number resonance = 0.0, "resonance", 0.0..=1.0;
    number resonance_velocity = 0.0, "resonanceVelocity", -1.0..=1.0;
    number drive = 0.0, "drive", 0.0..=1.0;
    number feedback = 0.0, "feedback", 0.0..=Filter::MAX_FEEDBACK;
    number fm = 0.0, "fmAmount", -1.0..=1.0;
    number env_amount = 0.0, "envAmount", -2.0..=2.0;
    number key_track = 0.0, "keyTrack", 0.0..=1.5;
    number mod_slew_ms = 0.0, "modSlewMs", 0.0..=Filter::MAX_LAG_MS;
    flag env_holds = false, "envHoldsVoice";
    nested env: Envelope = Envelope::FILTER, "env";
});

impl Filter {
    pub const MIN_HZ: f64 = 20.0;
    pub const MIN_CUTOFF_HZ: f64 = 5.0;
    pub const MAX_HZ: f64 = 20_000.0;
    pub const MAX_LAG_MS: f64 = 50.0;
    pub const MAX_FEEDBACK: f64 = 0.95;
    pub const OVERSAMPLES: [usize; 3] = [1, 2, 4];
}

fn oversample_of(times: f64) -> usize {
    if times >= 3.0 {
        4
    } else if times >= 1.5 {
        2
    } else {
        1
    }
}

const LADDER_BAND_PASS: &str = "ladder-band-pass";

fn filter_word(mode: FilterMode, shape: FilterShape) -> String {
    match mode {
        FilterMode::Svf => format!("svf-{}", shape.name()),
        FilterMode::Ladder if shape == FilterShape::BandPass => String::from(LADDER_BAND_PASS),
        mode => String::from(mode.name()),
    }
}

fn filter_choice(json: Option<&Json>) -> Option<(FilterMode, FilterShape)> {
    let word = name(json, "mode")?;
    if word == LADDER_BAND_PASS {
        return Some((FilterMode::Ladder, FilterShape::BandPass));
    }
    match word.strip_prefix("svf-") {
        Some(shape) => FilterShape::from_name(shape).map(|shape| (FilterMode::Svf, shape)),
        None => FilterMode::from_name(word).map(|mode| (mode, FilterShape::LowPass)),
    }
}

params!(Noise {
    switch on = false, "on";
    pick color: NoiseColor = NoiseColor::White, "color";
    lead level = 0.5, "level", 0.0..=1.0;
    number tone = 0.0, "tone", -1.0..=1.0;
    number width = 0.0, "width", 0.0..=1.0;
    number high_pass_hz = Filter::MIN_HZ, "highPassHz", Filter::MIN_HZ..=Filter::MAX_HZ;
    number low_pass_hz = Filter::MAX_HZ, "lowPassHz", Filter::MIN_HZ..=Filter::MAX_HZ;
});

params!(Analog {
    number detune = 0.0, "driftCents", 0.0..=Analog::MAX_CENTS;
    number wander = 0.0, "wanderCents", 0.0..=Analog::MAX_CENTS;
    number wander_rate = 1.0, "wanderRate", Analog::MIN_RATE..=Analog::MAX_RATE;
    number jitter = 0.0, "jitterCents", 0.0..=Analog::MAX_JITTER_CENTS;
    number cutoff_wander = 0.0, "cutoffWanderCents", 0.0..=Analog::MAX_CUTOFF_WANDER_CENTS;
    number width_wander = 0.0, "widthWander", 0.0..=Analog::MAX_WIDTH;
    number cutoff_scatter = 0.0, "cutoffScatterCents", 0.0..=Analog::MAX_CUTOFF_SCATTER_CENTS;
    number env_scatter = 0.0, "envScatter", 0.0..=1.0;
    number width_scatter = 0.0, "widthScatter", 0.0..=Analog::MAX_WIDTH;
    number level_scatter = 0.0, "levelScatterDb", 0.0..=Analog::MAX_LEVEL_DB;
    number pan_scatter = 0.0, "panScatter", 0.0..=1.0;
    number heat_pre = 0.0, "heatPre", 0.0..=1.0;
    number heat_post = 0.0, "heatPost", 0.0..=1.0;
    number lean = 0.0, "heatLean", 0.0..=1.0;
    number ac_hz = 0.0, "acHz", 0.0..=Analog::MAX_AC_HZ;
    flag bypass = false, "bypass";
});

impl Analog {
    pub const MAX_CENTS: f64 = 50.0;
    pub const MIN_RATE: f64 = 0.1;
    pub const MAX_RATE: f64 = 10.0;
    pub const MAX_JITTER_CENTS: f64 = 10.0;
    pub const MAX_CUTOFF_WANDER_CENTS: f64 = 600.0;
    pub const MAX_CUTOFF_SCATTER_CENTS: f64 = 1200.0;
    pub const MAX_WIDTH: f64 = 0.25;
    pub const MAX_LEVEL_DB: f64 = 6.0;
    pub const MAX_AC_HZ: f64 = 200.0;

    pub fn heard(&self) -> Analog {
        if self.bypass { Analog::DEFAULT } else { *self }
    }
}

params!(Modulation {
    number level1 = 0.0, "osc1Level", -2.0..=2.0;
    number level2 = 0.0, "osc2Level", -2.0..=2.0;
    number tune2 = 0.0, "osc2Semitones", -24.0..=24.0;
    number tune3 = 0.0, "osc3Semitones", -24.0..=24.0;
    number tune4 = 0.0, "osc4Semitones", -24.0..=24.0;
    number cutoff = 0.0, "cutoffOctaves", -5.0..=5.0;
    number pan = 0.0, "pan", -1.0..=1.0;
    number pulse_width = 0.0, "pulseWidth", -0.45..=0.45;
    number sample = 0.0, "sample", -1.0..=1.0;
    flag sampled = false, "sampled";
    choice quantize: Quantize = Quantize::Off, "quantize";
    number lag_ms = 0.0, "lagMs", 0.0..=5000.0;
});

impl Modulation {
    pub fn tunes(&self) -> [f64; Params::OSCS] {
        [0.0, self.tune2, self.tune3, self.tune4]
    }
}

params!(Lfo {
    choice target: Target = Target::Off, "target";
    number amount = 0.0, "amount", -1.0..=1.0;
    choice shape: Shape = Shape::Sine, "shape";
    flag sync = false, "sync";
    choice division: Division = Division::Quarter, "division";
    number rate_hz = 2.0, "rateHz", Lfo::MIN_HZ..=Lfo::MAX_HZ;
    number key_track = 0.0, "keyTrack", 0.0..=1.5;
    number phase = 0.0, "phase", 0.0..=1.0;
    choice trigger: Trigger = Trigger::Trig, "mode";
    custom points: face::Drawn = face::Drawn::RAMP, "points", face::Drawn::manual("2 to 32 points in x order, the drawn shape's cycle; y 0 the bottom of the swing, 1 the top; ends pinned to x 0 and 1; default a ramp 0 to 1, kept whatever the shape"), |p, d, f| if p.shape == Shape::Drawn || p.points != d.points { f.push("points", p.points.to_json()) }, |json, d| face::Drawn::from_json(items(json, "points")).unwrap_or(d.points);
});

impl Lfo {
    pub const MIN_HZ: f64 = 0.05;
    pub const MAX_HZ: f64 = 50.0;

    pub fn aimed(&self) -> bool {
        self.target != Target::Off
    }

    pub fn tempo(&mut self, bpm: f64) {
        if self.sync {
            self.division = Division::nearest(1000.0 / self.rate_hz, bpm, &Division::ALL);
        } else {
            self.rate_hz = self.division.hz(bpm).clamp(Lfo::MIN_HZ, Lfo::MAX_HZ);
        }
    }
}

params!(Mover {
    choice target: Target = Target::Off, "target";
    number amount = 0.0, "amount", -1.0..=1.0;
    nested env: Envelope = Envelope::MOTION, "env";
});

impl Mover {
    pub fn aimed(&self) -> bool {
        self.target != Target::Off
    }
}

params!(Motion {
    nested lfo1: Lfo = Lfo::DEFAULT, "lfo1";
    nested lfo2: Lfo = Lfo::DEFAULT, "lfo2";
    nested env1: Mover = Mover::DEFAULT, "env1";
    nested env2: Mover = Mover::DEFAULT, "env2";
});

impl Motion {
    pub const LFOS: usize = 2;
    pub const ENVS: usize = 2;

    pub fn lfos(&self) -> [Lfo; Motion::LFOS] {
        [self.lfo1, self.lfo2]
    }

    pub fn lfo_mut(&mut self, index: usize) -> &mut Lfo {
        if index == 0 { &mut self.lfo1 } else { &mut self.lfo2 }
    }

    pub fn envs(&self) -> [Mover; Motion::ENVS] {
        [self.env1, self.env2]
    }

    pub fn moves(&self, target: Target) -> bool {
        let aimed = |aim: Target| aim == target || aim.whole() == Some(target) || target.whole() == Some(aim);
        self.lfos().iter().any(|lfo| lfo.aimed() && aimed(lfo.target)) || self.envs().iter().any(|env| env.aimed() && aimed(env.target))
    }

    pub fn on(&self) -> bool {
        self.lfos().iter().any(Lfo::aimed) || self.envs().iter().any(Mover::aimed)
    }
}

params!(Mono {
    number glide_ms = 0.0, "glideMs", 0.0..=20_000.0;
    flag legato = true, "legato";
    flag retrigger = false, "retrigger";
    choice curve: GlideCurve = GlideCurve::Ease, "glideCurve";
});

params!(Params {
    whole voices: usize = 16, "voices", 1.0..=32.0;
    whole stack: usize = 1, "stack", 1.0..=Params::MAX_STACK as f64;
    number stack_cents = 0.0, "stackCents", 0.0..=Params::MAX_STACK_CENTS;
    number stack_spread = 0.0, "stackSpread", 0.0..=1.0;
    flag fresh = false, "freshVoice";
    nested osc1: Osc = Osc::with_level(1.0), "osc1";
    nested osc2: Osc = Osc::with_level(0.0), "osc2";
    nested osc3: Osc = Osc::with_level(0.0), "osc3";
    nested osc4: Osc = Osc::with_level(0.0), "osc4";
    choice modulator: Modulator = Modulator::Osc2, "modulator";
    number phase_random = 1.0, "phaseRandom", 0.0..=1.0;
    nested analog: Analog = Analog::DEFAULT, "analog";
    nested noise: Noise = Noise::DEFAULT, "noise";
    always amp: Envelope = Envelope::DEFAULT, "ampEnv";
    nested filter: Filter = Filter::DEFAULT, "filter";
    nested mono: Mono = Mono::DEFAULT, "mono";
    nested modulation: Modulation = Modulation::DEFAULT, "mod";
    nested motion: Motion = Motion::DEFAULT, "motion";
});

pub const HERTZ: Scale = Scale::Log { lo: Filter::MIN_HZ, hi: Filter::MAX_HZ };
pub const MILLIS: Scale = Scale::Log { lo: Envelope::MIN_MS, hi: Envelope::MAX_MS };
const ATTACK_MILLIS: Scale = Scale::Log { lo: Envelope::MIN_ATTACK_MS, hi: Envelope::MAX_MS };
const CUTOFF_HZ: Scale = Scale::Log { lo: Filter::MIN_CUTOFF_HZ, hi: Filter::MAX_HZ };
pub const WAIT: Scale = skewed(0.0, Envelope::MAX_WAIT_MS, 4.0);
const ANALOG_CENTS: Scale = skewed(0.0, Analog::MAX_CENTS, 2.65);

bank!(pub OscKnobs: Params { level, wave1, wave2, wave3, wave4, tune, fine, unison, detune, width, pulse_width, phase, fm, sync_phase, step_share, blend });
bank!(pub EnvelopeKnobs: Params { attack, decay, sustain, release, velocity, delay, key_follow });
bank!(pub LfoKnobs: Params { amount, rate, phase, keytrack });
bank!(pub MotionAmounts: Params { env1, env2 });

macro_rules! osc {
    ($osc:ident, $band:literal) => {
        OscKnobs { level: Param::new($band, "LEVEL", field!($osc.level), UNIT), wave1: Param::new($band, "WAVE 1", field!($osc.levels[0]), UNIT), wave2: Param::new($band, "WAVE 2", field!($osc.levels[1]), UNIT), wave3: Param::new($band, "WAVE 3", field!($osc.levels[2]), UNIT), wave4: Param::new($band, "WAVE 4", field!($osc.levels[3]), UNIT), tune: Param::new($band, "TUNE", field!(|p| p.$osc.tune, |p, v| p.$osc.tune = v.round()), linear(-48.0, 48.0)), fine: Param::new($band, "FINE", field!($osc.fine), linear(-100.0, 100.0)), unison: Param::new($band, "UNISON", int!($osc.unison), linear(1.0, 16.0)), detune: Param::new($band, "DETUNE", field!($osc.detune), skewed(0.0, Osc::MAX_DETUNE_CENTS, 3.5)), width: Param::new($band, "WIDTH", field!($osc.width), UNIT), pulse_width: Param::new($band, "PULSE WIDTH", field!($osc.pulse_width), linear(0.05, 0.95)), phase: Param::new($band, "PHASE", field!($osc.phase), UNIT), fm: Param::new($band, "FM", field!($osc.fm), UNIT), sync_phase: Param::new($band, "SYNC PHASE", field!($osc.sync_phase), linear(-0.5, 1.0)), step_share: Param::new($band, "STEP SHARE", field!($osc.step_share), BIPOLAR), blend: Param::new($band, "BLEND", field!($osc.blend), UNIT) }
    };
}

macro_rules! envelope {
    ($band:literal, $($path:tt)+) => {
        EnvelopeKnobs { attack: Param::<Params>::new($band, "ATTACK", field!($($path)+.attack_ms), ATTACK_MILLIS).swung(MILLIS), decay: Param::new($band, "DECAY", field!($($path)+.decay_ms), MILLIS), sustain: Param::new($band, "SUSTAIN", field!($($path)+.sustain), UNIT), release: Param::new($band, "RELEASE", field!($($path)+.release_ms), MILLIS), velocity: Param::new($band, "VELOCITY", field!($($path)+.velocity), UNIT), delay: Param::new($band, "DELAY", field!($($path)+.delay_ms), WAIT), key_follow: Param::new($band, "KEY FOLLOW", field!($($path)+.key_follow), BIPOLAR) }
    };
}

macro_rules! lfo {
    ($lfo:ident, $band:literal) => {
        LfoKnobs { amount: Param::new($band, "AMOUNT", field!(motion.$lfo.amount), BIPOLAR), rate: Param::new($band, "RATE", field!(motion.$lfo.rate_hz), log(Lfo::MIN_HZ, Lfo::MAX_HZ)), phase: Param::new($band, "PHASE", field!(motion.$lfo.phase), UNIT), keytrack: Param::new($band, "KEYTRACK", field!(motion.$lfo.key_track), linear(0.0, 1.5)) }
    };
}

pub const OSC1: OscKnobs = osc!(osc1, "OSC 1");
pub const OSC2: OscKnobs = osc!(osc2, "OSC 2");
pub const OSC3: OscKnobs = osc!(osc3, "OSC 3");
pub const OSC4: OscKnobs = osc!(osc4, "OSC 4");
pub const AMP: EnvelopeKnobs = envelope!("AMP ENV", amp);
pub const FILTER_ENV: EnvelopeKnobs = envelope!("FILTER ENV", filter.env);
pub const LFO1: LfoKnobs = lfo!(lfo1, "LFO 1");
pub const LFO2: LfoKnobs = lfo!(lfo2, "LFO 2");
pub const MOTION_ENV1: EnvelopeKnobs = envelope!("ENV 1", motion.env1.env);
pub const MOTION_ENV2: EnvelopeKnobs = envelope!("ENV 2", motion.env2.env);
pub const MOTION_AMOUNTS: MotionAmounts = MotionAmounts { env1: Param::new("ENV 1", "AMOUNT", field!(motion.env1.amount), BIPOLAR), env2: Param::new("ENV 2", "AMOUNT", field!(motion.env2.amount), BIPOLAR) };
pub const DIVISIONS: &[&str] = &division::LABELS;
table! {
    REST: Params {
        PHASE_RANDOM = "UNISON", "PHASE RANDOM", field!(phase_random), UNIT;
        LEVEL = "NOISE", "LEVEL", field!(noise.level), UNIT;
        WIDTH = "NOISE", "WIDTH", field!(noise.width), UNIT;
        TONE = "NOISE", "TONE", field!(noise.tone), BIPOLAR;
        NOISE_HIGH_PASS = "NOISE", "HIGH PASS", field!(noise.high_pass_hz), HERTZ;
        NOISE_LOW_PASS = "NOISE", "LOW PASS", field!(noise.low_pass_hz), HERTZ;
        CUTOFF = "FILTER", "CUTOFF", field!(filter.cutoff_hz), CUTOFF_HZ => HERTZ;
        RESONANCE = "FILTER", "RESONANCE", field!(filter.resonance), UNIT;
        RESONANCE_VELOCITY = "FILTER", "RES VELOCITY", field!(filter.resonance_velocity), BIPOLAR;
        ENV_AMOUNT = "FILTER", "ENV AMOUNT", field!(filter.env_amount), linear(-2.0, 2.0);
        KEYTRACK = "FILTER", "KEYTRACK", field!(filter.key_track), linear(0.0, 1.5);
        DRIVE = "FILTER", "DRIVE", field!(filter.drive), UNIT;
        FEEDBACK = "FILTER", "FEEDBACK", field!(filter.feedback), linear(0.0, Filter::MAX_FEEDBACK);
        FILTER_FM = "FILTER", "FM", field!(filter.fm), BIPOLAR;
        MOD_SLEW = "FILTER", "MOD SLEW", field!(filter.mod_slew_ms), skewed(0.0, Filter::MAX_LAG_MS, 2.0);
        DETUNE = "ANALOG", "TUNE SCATTER", field!(analog.detune), ANALOG_CENTS;
        DRIFT = "ANALOG", "DRIFT", field!(analog.wander), ANALOG_CENTS;
        DRIFT_RATE = "ANALOG", "DRIFT RATE", field!(analog.wander_rate), log(Analog::MIN_RATE, Analog::MAX_RATE);
        JITTER = "ANALOG", "JITTER", field!(analog.jitter), skewed(0.0, Analog::MAX_JITTER_CENTS, 2.0);
        CUTOFF_DRIFT = "ANALOG", "CUTOFF DRIFT", field!(analog.cutoff_wander), skewed(0.0, Analog::MAX_CUTOFF_WANDER_CENTS, 3.0);
        WIDTH_DRIFT = "ANALOG", "WIDTH DRIFT", field!(analog.width_wander), linear(0.0, Analog::MAX_WIDTH);
        CUTOFF_SCATTER = "ANALOG", "CUTOFF SCATTER", field!(analog.cutoff_scatter), skewed(0.0, Analog::MAX_CUTOFF_SCATTER_CENTS, 3.0);
        ENV_SCATTER = "ANALOG", "ENV SCATTER", field!(analog.env_scatter), UNIT;
        WIDTH_SCATTER = "ANALOG", "WIDTH SCATTER", field!(analog.width_scatter), linear(0.0, Analog::MAX_WIDTH);
        LEVEL_SCATTER = "ANALOG", "LEVEL SCATTER", field!(analog.level_scatter), skewed(0.0, Analog::MAX_LEVEL_DB, 2.0);
        PAN_SCATTER = "ANALOG", "PAN SCATTER", field!(analog.pan_scatter), UNIT;
        HEAT_PRE = "ANALOG", "HEAT PRE", field!(analog.heat_pre), UNIT;
        HEAT_POST = "ANALOG", "HEAT POST", field!(analog.heat_post), UNIT;
        HEAT_LEAN = "ANALOG", "HEAT LEAN", field!(analog.lean), UNIT;
        AC = "ANALOG", "LOW CUT", field!(analog.ac_hz), skewed(0.0, Analog::MAX_AC_HZ, 2.0);
        VOICES = "VOICE", "VOICES", int!(voices), linear(1.0, 32.0);
        STACK = "VOICE", "STACK", int!(stack), linear(1.0, Params::MAX_STACK as f64);
        STACK_CENTS = "VOICE", "STACK CENTS", field!(stack_cents), linear(0.0, Params::MAX_STACK_CENTS);
        STACK_SPREAD = "VOICE", "STACK SPREAD", field!(stack_spread), UNIT;
        GLIDE = "MONO", "GLIDE", field!(mono.glide_ms), skewed(0.0, 20_000.0, 5.0);
        MOD_LEVEL1 = "MOD", "OSC 1 LEVEL", field!(modulation.level1), linear(-2.0, 2.0);
        MOD_LEVEL2 = "MOD", "OSC 2 LEVEL", field!(modulation.level2), linear(-2.0, 2.0);
        MOD_TUNE2 = "MOD", "OSC 2 TUNE", field!(|p| p.modulation.tune2, |p, v| p.modulation.tune2 = v.round()), linear(-24.0, 24.0);
        MOD_TUNE3 = "MOD", "OSC 3 TUNE", field!(|p| p.modulation.tune3, |p, v| p.modulation.tune3 = v.round()), linear(-24.0, 24.0);
        MOD_TUNE4 = "MOD", "OSC 4 TUNE", field!(|p| p.modulation.tune4, |p, v| p.modulation.tune4 = v.round()), linear(-24.0, 24.0);
        MOD_CUTOFF = "MOD", "CUTOFF", field!(modulation.cutoff), linear(-5.0, 5.0);
        MOD_PAN = "MOD", "PAN", field!(modulation.pan), BIPOLAR;
        MOD_PULSE_WIDTH = "MOD", "PULSE WIDTH", field!(modulation.pulse_width), linear(-0.45, 0.45);
        MOD_LAG = "MOD", "LAG", field!(modulation.lag_ms), skewed(0.0, 5000.0, 4.0);
        MOD_SAMPLE = "MOD", "SAMPLE", field!(modulation.sample), BIPOLAR;
    }
}

static FLAT: [Param<Params>; 147] = flat(&[&OSC1.all(), &OSC2.all(), &OSC3.all(), &OSC4.all(), &AMP.all(), &FILTER_ENV.all(), &LFO1.all(), &LFO2.all(), &MOTION_ENV1.all(), &MOTION_ENV2.all(), &MOTION_AMOUNTS.all(), REST]);

pub static PARAMS: &[Param<Params>] = &FLAT;

pub fn knobs() -> &'static [Knob] {
    static KNOBS: OnceLock<Vec<Knob>> = OnceLock::new();
    KNOBS.get_or_init(|| param::knobs(&listed(), Params::DEFAULT, spelled))
}

pub fn manual(fields: &mut Fields) {
    let described = face::described(&CORE, Params::DEFAULT, spelled);
    let describe = |path: &str| keyed_description(path).map_or_else(|| face::description(&described, path), String::from);
    Params::manual(&Params::DEFAULT, "", &describe, fields);
    fields.push("lanes", param::lanes(knobs()));
}

fn keyed_description(path: &str) -> Option<&'static str> {
    let (group, key) = path.rsplit_once('.').unwrap_or(("", path));
    Some(match (group, key) {
        (_, "attackBend") => "curves the rise; 0 straight, 1 fast then slow, -1 slow then fast",
        (_, "decayBend" | "releaseBend") => "curves the fall; 0 straight, -1 fast then slow, 1 slow then fast",
        (_, "keyFollow") => "decay and release follow the key: 1 halves them each octave above C4, doubles below; attack stays",
        ("", "stack") => "voices each note plays, each with its own phases and noise, counted against voices",
        ("", "modulator") => "osc2 or osc3 modulates the others; its own fmAmount and hardSync are disabled",
        ("", "phaseRandom") => "unison start-phase scatter around each oscillator's phase: 0 aligned, 1 random",
        ("osc1" | "osc2" | "osc3" | "osc4", "fmAmount") => "phase modulation depth; linearFm switches to frequency modulation",
        ("osc1" | "osc2" | "osc3" | "osc4", "linearFm") => "linear frequency modulation; at fmAmount 1, deviation reaches +/-100%",
        ("osc1" | "osc2" | "osc3" | "osc4", "stepShare") => "share of an arp step's semitones and octave shift the oscillator follows: 1 whole, 0 none, staying on the held key, -1 turned over",
        ("osc1" | "osc2" | "osc3" | "osc4", "unisonBlend") => "level of the detuned copies beside the centre copy; 1 equal, 0 the centre alone",
        ("osc1" | "osc2" | "osc3" | "osc4", "syncPhase") => "hard-sync reset phase: 0 is the top; negative values restart below it",
        ("analog", "envScatter") => "random envelope speed per note; 1 spans half to double",
        ("analog", "bypass") => "switches all of analog off, to compare",
        ("filter", "slopeDbPerOctave") => "slope in dB per octave; ladder-band-pass takes 12 or 24, and 18 gives it 24",
        ("filter", "oversample") => "oversampling for the svf and ladder modes; the cutoff ceiling is 0.45, 0.9 or 1.8 times the sample rate",
        ("filter", "combRead") => "linear damps upper harmonics; allpass keeps their level",
        ("filter", "resonanceVelocity") => "velocity adds resonance; 1 spans its range, negative subtracts",
        ("filter", "feedback") => "positive feedback before drive; raises passband gain, flattens the knee",
        ("filter", "fmAmount") => "audio-rate cutoff modulation: 1 spans five octaves; negative inverts",
        ("filter", "modSlewMs") => "slews cutoff modulation from the filter envelope, motion and a note's mod; 0 is instant",
        ("mono", "glideCurve") => "ease settles exponentially; linear takes glideMs per slide, octave per octave",
        ("mod", "osc1Level") => "how far a note's mod moves osc 1's level; 1 spans its full range",
        ("mod", "osc2Level") => "how far a note's mod moves osc 2's level; 1 spans its full range",
        ("mod", "osc2Semitones") => "osc 2's pitch offset at a note mod of 1, in semitones",
        ("mod", "osc3Semitones") => "osc 3's pitch offset at a note mod of 1, in semitones",
        ("mod", "osc4Semitones") => "osc 4's pitch offset at a note mod of 1, in semitones",
        ("mod", "cutoffOctaves") => "filter cutoff offset at a note mod of 1, in octaves; the filter's modSlewMs slews it",
        ("mod", "pan") => "pan offset at a note mod of 1; 1 is hard right, -1 hard left",
        ("mod", "pulseWidth") => "pulse width offset at a note mod of 1, on an oscillator sounding square alone; 0.45 reaches either end from the middle",
        ("mod", "sample") => "the mod value each note takes at its start when sampled is true; a lane can move it",
        ("mod", "sampled") => "each note holds sample as read at its start, scaled by its own mod or its arp step's",
        ("mod", "quantize") => "snaps the pitch offsets from osc2Semitones, osc3Semitones and osc4Semitones to the chosen intervals, chord or scale, before lagMs",
        ("mod", "lagMs") => "glide time from the previous note's mod value",
        ("motion.lfo1" | "motion.lfo2", "target") => "per-voice LFO target; the target's own value is the centre",
        ("motion.lfo1" | "motion.lfo2" | "motion.env1" | "motion.env2", "amount") => "1 moves pitch 12 semitones, cutoff 5 octaves, pulse width 0.45, LFO rate 10 octaves, others their full range",
        ("motion.lfo1" | "motion.lfo2", "shape") => "random holds each cycle; walk glides; chaos wanders; drawn follows points",
        ("motion.lfo1" | "motion.lfo2", "sync") => "true uses division; false uses rateHz",
        ("motion.lfo1" | "motion.lfo2", "rateHz") => "free rate in Hz; a target of lfo1Rate or lfo2Rate moves the rate, synced or free",
        ("motion.lfo1" | "motion.lfo2", "keyTrack") => "rate follows the key, synced or free: 1 doubles it each octave above C4, halves below",
        ("motion.lfo1" | "motion.lfo2", "mode") => "trig restarts per note; run shares one wave; once plays then holds",
        ("motion.env1" | "motion.env2", "target") => "per-voice envelope target; the target's own value is the rest, amount the peak",
        _ => return None,
    })
}

fn spelled(patch: &Params, against: &Params) -> Json {
    let mut fields = Fields::new();
    patch.fields(against, &mut fields);
    fields.into_json()
}

fn listed() -> Vec<Param<Params>> {
    PARAMS.iter().copied().chain(face::switches(&CORE)).collect()
}

fn param_of(band: &str, name: &str) -> Option<&'static Param<Params>> {
    param::find(PARAMS, band, name).or_else(|| face::switched(&CORE, band, name).map(|cell| &cell.param))
}

impl Params {
    pub const OSCS: usize = 4;
    pub const MAX_STACK: usize = 4;
    pub const MAX_STACK_CENTS: f64 = 100.0;

    pub fn oscs(&self) -> [Osc; Params::OSCS] {
        [self.osc1, self.osc2, self.osc3, self.osc4]
    }

    pub fn osc_mut(&mut self, index: usize) -> &mut Osc {
        match index {
            0 => &mut self.osc1,
            1 => &mut self.osc2,
            2 => &mut self.osc3,
            _ => &mut self.osc4,
        }
    }

    pub fn automatable(band: &str, name: &str) -> bool {
        param_of(band, name).is_some()
    }

    pub fn lane_share(band: &str, name: &str) -> f64 {
        param_of(band, name).map_or(1.0, Param::lane_share)
    }

    pub fn lane_seed(band: &str, name: &str) -> Option<u64> {
        param_of(band, name).map(|param| param.seed)
    }

    pub fn travel_of(&self, band: &str, name: &str) -> Option<f64> {
        param_of(band, name).map(|param| param.travel_of(self))
    }

    pub fn set_travel(&mut self, band: &str, name: &str, travel: f64) -> bool {
        match param_of(band, name) {
            Some(param) => {
                param.set_travel(self, travel);
                true
            }
            None => false,
        }
    }
}

impl Patch for Params {
    fn factory(&self) -> Params {
        Params::DEFAULT
    }
}

impl Target {
    pub const fn whole(self) -> Option<Target> {
        match self {
            Target::Pitch1 | Target::Pitch2 | Target::Pitch3 | Target::Pitch4 => Some(Target::Pitch),
            Target::Fm1 | Target::Fm2 | Target::Fm3 | Target::Fm4 => Some(Target::Fm),
            _ => None,
        }
    }

    pub const fn fm_of(osc: usize) -> Target {
        [Target::Fm1, Target::Fm2, Target::Fm3, Target::Fm4][osc]
    }
}

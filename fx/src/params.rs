use sdk::schema::{Fields, MAX_TIMES, bounded, choice, either, flag, index, items, name, number, one_of, params};
use sdk::division::Division;
use crate::{Kind, Params};
use sdk::json::{Json, tidy};
use sdk::param::{BIPOLAR, LIMITER_RELEASE_MS, Param, Scale, UNIT, bank, field, flat, floored, int, linear, log, release_ms, skewed, table};
pub use crate::lfo_shape::VolumeLfo;
pub use sdk::dsp::filter::{Filter, Pass, Slope};

bank!(pub FilterKnobs<P> { freq, q });

macro_rules! filter_params {
    ($band:literal, $($unit:tt)+) => {
        FilterKnobs { freq: Param::new($band, "FREQ", field!($($unit)+.hz), log(Filter::MIN_HZ, Filter::MAX_HZ)), q: Param::new($band, "Q", field!($($unit)+.q), log(Filter::MIN_Q, Filter::MAX_Q)) }
    };
}

pub trait Filtered {
    fn sites(&self) -> &'static [&'static [usize]];

    fn filter(&self, unit: usize) -> &Filter;

    fn filter_mut(&mut self, unit: usize) -> &mut Filter;

    fn site(&self, unit: usize) -> Vec<(usize, Filter)> {
        let units = self.sites().iter().find(|site| site.contains(&unit)).copied().unwrap_or(&[]);
        units.iter().map(|&unit| (unit, *self.filter(unit))).collect()
    }
}

pub mod chorus;
pub mod compressor;
pub mod delay;
pub mod eq;
pub mod flanger;
pub mod freak;
pub mod gate;
pub mod motion;
pub mod phaser;
pub mod repeat;
pub mod reverb;
pub mod sidechain;
pub mod space;
pub mod suppressor;
pub mod time;
pub mod transient;
pub mod utility;
pub mod volume_lfo;

pub use chorus::{Chorus, ChorusMode};
pub use compressor::Compressor;
pub use delay::{Delay, DelayMode};
pub use eq::{BandPass, Channel, Eq, EqBand, PassBand, Shape};
pub use flanger::{Flanger, FlangerMode, FlangerShape};
pub use freak::{BandStereo, Curve, DetectMode, DetectSlope, DetectStereo, FLIP_LABELS, Freak, FreakSlope, RippleShape};
pub use gate::Gate;
pub use motion::{Motion, MotionMode};
pub use phaser::{Phaser, PhaserShape, Routing, STAGES, STAGE_LABELS};
pub use repeat::{Grid, INTERVALS, INTERVAL_LABELS, Repeat, RepeatMix, Vary};
pub use reverb::{Age, EarlyPattern, Reverb, Tank};
pub use sidechain::Sidechain;
pub use space::Space;
pub use suppressor::{Suppressor, SuppressorBand};
pub use time::Time;
pub use transient::Transient;
pub use utility::{LookaheadCurve, Utility};

pub const EQ_BANDS: usize = 8;
pub const FREAK_BANDS: usize = 4;
pub const MOTION_BANDS: usize = 4;
pub const SUPPRESSOR_BANDS: usize = 4;
pub const PASS_BANDS: usize = 3;
pub const RATE_HZ: Scale = Scale::Log { lo: RATE_MIN_HZ, hi: RATE_MAX_HZ };
pub const CURVE_LO_HZ: f64 = 20.0;
pub const CURVE_HI_HZ: f64 = 20_000.0;
pub(crate) const HALFBAND_LATENCY: usize = 2;

choice!(pub Lookahead { Off = "Off" / "off", Ms1 = "1 ms" / "1ms", Ms5 = "5 ms" / "5ms" });

impl Lookahead {
    pub fn samples(self, sample_rate: f64) -> usize {
        let ms = match self {
            Lookahead::Off => 0.0,
            Lookahead::Ms1 => 1.0,
            Lookahead::Ms5 => 5.0,
        };
        (ms * 0.001 * sample_rate).round() as usize
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Signal {
    pub source: f64,
    pub pre: bool,
}

impl Signal {
    pub const OFF: Signal = Signal { source: 0.0, pre: false };
    pub const SOURCE_TRACK: &'static str = "sourceTrack";
    pub const PRE_EFFECTS: &'static str = "preEffects";

    fn new(track: Option<usize>, pre: bool) -> Signal {
        Signal { source: track.and_then(|track| track.checked_add(1)).map_or(0.0, |track| track as f64), pre }
    }

    fn track(&self) -> Option<usize> {
        (self.source.round() as usize).checked_sub(1)
    }

    fn fields(&self, fields: &mut Fields) {
        if let Some(track) = self.track() {
            fields.push(Signal::SOURCE_TRACK, Json::Number(track as f64));
        }
        fields.flag(Signal::PRE_EFFECTS, self.pre, false);
    }

    fn from_json(json: &Json) -> Signal {
        Signal::new(index(Some(json), Signal::SOURCE_TRACK, usize::MAX), flag(Some(json), Signal::PRE_EFFECTS, false))
    }
}

impl Params {
    pub(crate) fn signal(&self) -> Option<&Signal> {
        match self {
            Params::Gate(Gate { signal, .. }) | Params::Sidechain(Sidechain { signal, .. }) | Params::Space(Space { signal, .. }) => Some(signal),
            _ => None,
        }
    }

    fn signal_mut(&mut self) -> Option<&mut Signal> {
        match self {
            Params::Gate(Gate { signal, .. }) | Params::Sidechain(Sidechain { signal, .. }) | Params::Space(Space { signal, .. }) => Some(signal),
            _ => None,
        }
    }

    pub(crate) fn filtered(&self) -> Option<&dyn Filtered> {
        match self {
            Params::Delay(p) => Some(p),
            Params::Freak(p) => Some(p),
            Params::Motion(p) => Some(p),
            Params::Repeat(p) => Some(p),
            Params::Reverb(p) => Some(p),
            _ => None,
        }
    }

    pub(crate) fn filtered_mut(&mut self) -> Option<&mut dyn Filtered> {
        match self {
            Params::Delay(p) => Some(p),
            Params::Freak(p) => Some(p),
            Params::Motion(p) => Some(p),
            Params::Repeat(p) => Some(p),
            Params::Reverb(p) => Some(p),
            _ => None,
        }
    }

    pub fn latency(&self, sample_rate: f64) -> usize {
        match self {
            Params::Utility(p) => p.latency(sample_rate),
            Params::Compressor(p) => p.lookahead.samples(sample_rate),
            Params::Gate(p) => p.lookahead.samples(sample_rate),
            Params::Sidechain(p) => p.lookahead.samples(sample_rate),
            Params::Space(_) => crate::space::engine::LATENCY,
            Params::Time(_) => time::Time::LATENCY,
            Params::Suppressor(p) => p.latency(sample_rate),
            Params::Freak(p) => p.latency(sample_rate),
            Params::Motion(p) => p.latency(sample_rate),
            _ => 0,
        }
    }

    pub fn depth(&self, band: &str, name: &str, sample_rate: f64) -> usize {
        let at = |control: (&str, &str)| control == (band, name);
        match self {
            Params::Compressor(p) if at((compressor::MAKEUP.band, compressor::MAKEUP.name)) || at(("OUTPUT", "SOFT CLIP")) => p.lookahead.samples(sample_rate),
            Params::Freak(p) if p.slope == FreakSlope::Linear && p.count() > 1 && !at((freak::SUB_CUT.band, freak::SUB_CUT.name)) && !at(("MASTER", "SUB CUT SLOPE")) => sdk::dsp::crossover::half(sample_rate),
            Params::Motion(p) if p.slope == FreakSlope::Linear && p.count() > 1 => sdk::dsp::crossover::half(sample_rate),
            Params::Suppressor(p) if name == suppressor::BAND_2_CROSSOVER.name => p.lookahead(sample_rate),
            Params::Time(_) => self.latency(sample_rate),
            _ => 0,
        }
    }
}

fn nested(write: impl FnOnce(&mut Fields)) -> Json {
    let mut fields = Fields::new();
    write(&mut fields);
    fields.into_object().unwrap_or(Json::Object(Vec::new()))
}

fn crossovers<const N: usize>(json: Option<&Json>, default: [f64; N], floor: f64, ceiling: f64) -> [f64; N] {
    let mut hz = default;
    for (slot, item) in hz.iter_mut().zip(items(json, "crossoversHz")) {
        *slot = item.as_f64().unwrap_or(*slot).clamp(floor, ceiling);
    }
    hz
}

pub const RATE_MIN_HZ: f64 = 0.05;
pub const RATE_MAX_HZ: f64 = 32.0;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Slot {
    pub wet: f64,
    pub bypass: bool,
    pub times: usize,
    pub params: Params,
}

impl Slot {
    pub fn new(kind: Kind) -> Slot {
        Slot { wet: 1.0, bypass: false, times: 1, params: Params::default_for(kind) }
    }

    pub fn kind(&self) -> Kind {
        self.params.kind()
    }

    pub fn latency(&self, sample_rate: f64) -> usize {
        if self.bypass { 0 } else { self.params.latency(sample_rate) }
    }

    pub fn depth(&self, band: &str, name: &str, sample_rate: f64) -> usize {
        if self.bypass { 0 } else { self.params.depth(band, name, sample_rate) }
    }

    pub fn mix_depth(&self, sample_rate: f64) -> usize {
        match &self.params {
            Params::Suppressor(p) if !self.bypass => p.lookahead(sample_rate),
            _ => self.latency(sample_rate),
        }
    }

    pub fn reverse_band(&self) -> Option<&'static str> {
        match &self.params {
            Params::Reverb(_) => Some("SPACE"),
            Params::Delay(_) => Some("TIME"),
            _ => None,
        }
    }

    pub fn reverse_level(&self) -> f64 {
        match &self.params {
            Params::Reverb(p) => p.reverse,
            Params::Delay(p) => p.reverse,
            _ => 0.0,
        }
    }

    pub fn blended(&self, forward: f64, reverse: f64) -> Slot {
        let params = match self.params {
            Params::Reverb(p) => Params::Reverb(Reverb { forward, reverse, dry: 0.0, ..p }),
            Params::Delay(p) => Params::Delay(Delay { forward, reverse, dry: 0.0, follow_dry: 0.0, ..p }),
            other => other,
        };
        Slot { params, ..*self }
    }

    pub fn forward(&self) -> Slot {
        Slot { wet: 1.0, bypass: false, ..self.blended(1.0, 0.0) }
    }

    pub fn wiring(&self) -> Option<(Option<usize>, bool)> {
        self.params.signal().map(|signal| (signal.track(), signal.pre))
    }

    pub fn wire(&mut self, source: Option<usize>, pre: bool) {
        if let Some(signal) = self.params.signal_mut() {
            *signal = Signal::new(source, pre);
        }
    }

    pub fn source(&self) -> Option<(usize, bool)> {
        self.wiring().and_then(|(source, pre)| source.map(|track| (track, pre)))
    }

    pub fn renumber(&mut self, map: impl Fn(usize) -> Option<usize>) {
        if let Some((Some(source), pre)) = self.wiring() {
            self.wire(map(source), pre);
        }
    }

    pub fn unsignal_gate(&mut self, from: impl Fn(usize) -> bool) {
        if let Some((Some(source), pre)) = self.wiring()
            && self.kind() == Kind::Gate
            && from(source)
        {
            self.wire(None, pre);
        }
    }

    pub fn to_json(&self) -> Json {
        nested(|fields| {
            fields.push("kind", Json::String(String::from(self.kind().name())));
            fields.number("wet", self.wet, 1.0);
            fields.flag("bypass", self.bypass, false);
            fields.number("times", self.times as f64, 1.0);
            self.params.fields(fields);
            if let Some(signal) = self.params.signal() {
                signal.fields(fields);
            }
        })
    }

    pub fn from_json(json: &Json) -> Option<Slot> {
        let kind = Kind::from_name(json.get("kind")?.as_str()?)?;
        let times = number(Some(json), "times", 1.0).round().clamp(1.0, MAX_TIMES as f64) as usize;
        let mut slot = Slot { wet: number(Some(json), "wet", 1.0).clamp(0.0, 1.0), bypass: flag(Some(json), "bypass", false), times, params: Params::from_json(kind, Some(json)) };
        if let Some(signal) = slot.params.signal_mut() {
            *signal = Signal::from_json(json);
        }
        Some(slot)
    }

    pub fn manual(kind: Kind, fields: &mut Fields) {
        fields.push("kind", Json::from(kind.name()));
        fields.manual("wet", bounded(1.0, 0.0..=1.0, false), "how much of the row's output is the effect, the rest dry");
        fields.manual("bypass", either(false), "the row passes sound through untouched");
        fields.manual("times", bounded(1.0, 1.0..=MAX_TIMES as f64, true), "the row sounds as this many copies of itself one after the other, each with the same settings, lanes and signal and its own state; its latency is multiplied, and every copy counts toward the rack's rows");
        kind.manual(fields);
    }
}

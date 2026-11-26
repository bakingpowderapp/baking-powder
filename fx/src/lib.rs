mod band_pass;
mod chorus;
mod compressor;
mod delay;
mod eq;
mod face;
mod flanger;
mod freak;
mod gate;
mod keyed;
mod lfo_shape;
mod motion;
pub mod params;
mod phaser;
mod picture;
mod repeat;
mod reverb;
mod sidechain;
mod silence;
mod space;
mod split;
mod suppressor;
mod time;
mod transient;
mod utility;
mod volume_lfo;

use sdk::schema::Fields;
use sdk::division::Division;
use sdk::json::Json;
use sdk::param::{self, Knob, Patch};
pub use params::Slot;
pub use picture::Picture;
pub use face::Readings;
pub use sidechain::{Heard, duck};
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Clock {
    pub sample_rate: f64,
    pub bpm: f64,
    pub beat: Option<Beat>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Beat {
    pub anchor: f64,
    pub since: i64,
    pub frames_per_beat: f64,
}

impl Beat {
    pub fn at(&self, frame: usize) -> f64 {
        self.anchor + (self.since + frame as i64) as f64 / self.frames_per_beat
    }
}

impl Clock {
    pub fn hz(&self, division: Division) -> f64 {
        division.hz(self.bpm).min(params::RATE_MAX_HZ)
    }

    pub fn ms(&self, division: Division) -> f64 {
        division.ms(self.bpm)
    }

    pub fn rate(&self, sync: bool, division: Division, rate_hz: f64) -> f64 {
        if sync { self.hz(division) } else { rate_hz }
    }
}

pub trait Unit {
    type Params;

    fn new(sample_rate: f64, max_frames: usize) -> Self;

    fn process(&mut self, l: &mut [f32], r: &mut [f32], params: &Self::Params, wet: f32, clock: &Clock);

    fn restart(&mut self) {}

    fn holding(&self) -> bool {
        false
    }

    fn feed(&mut self, _: &[f32], _: &[f32]) {}

    fn starve(&mut self) {}

    fn gauge(&self) -> f32 {
        0.0
    }

    fn signal(&self) -> f32 {
        f32::NEG_INFINITY
    }

    fn strips(&self) -> [[f32; 2]; params::FREAK_BANDS] {
        [[0.0; 2]; params::FREAK_BANDS]
    }

    fn curve(&self) -> Option<&[f32; sdk::spectrum::CURVE]> {
        None
    }
}

macro_rules! effects {
    ($($kind:ident: $module:ident, $label:literal, $name:literal;)+) => {
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum Kind {
            $($kind,)+
        }

        impl Kind {
            pub const ALL: [Kind; Kind::COUNT] = [$(Kind::$kind,)+];
            const COUNT: usize = [$($name,)+].len();
            const LABELS: [&'static str; Kind::COUNT] = [$($label,)+];
            const NAMES: [&'static str; Kind::COUNT] = [$($name,)+];

            pub const fn label(self) -> &'static str {
                Kind::LABELS[self as usize]
            }

            pub fn name(self) -> &'static str {
                Kind::NAMES[self as usize]
            }

            pub fn from_name(name: &str) -> Option<Kind> {
                Kind::ALL.into_iter().find(|kind| kind.name() == name)
            }

            pub fn knobs(self) -> &'static [Knob] {
                static KNOBS: OnceLock<Vec<Vec<Knob>>> = OnceLock::new();
                let spelled = KNOBS.get_or_init(|| {
                    vec![$(param::knobs(&face::params_of::<params::$kind>(params::$module::PARAMS), params::$kind::DEFAULT, spelled::<params::$kind>),)+]
                });
                &spelled[self as usize]
            }

            pub(crate) fn manual(self, fields: &mut Fields) {
                match self {
                    $(Kind::$kind => {
                        let described = sdk::face::described(<params::$kind as face::Cells>::CELLS, params::$kind::DEFAULT, spelled::<params::$kind>);
                        params::$kind::manual(&params::$kind::DEFAULT, "", &|path| keyed::description(self, path).map_or_else(|| sdk::face::description(&described, path), String::from), fields);
                        fields.push("lanes", param::lanes(self.knobs()));
                    })+
                }
            }
        }

        fn spelled<P: Spelled>(patch: &P, against: &P) -> Json {
            let mut fields = Fields::new();
            patch.fields(against, &mut fields);
            fields.into_json()
        }

        trait Spelled {
            fn fields(&self, default: &Self, fields: &mut Fields);
        }

        $(impl Spelled for params::$kind {
            fn fields(&self, default: &Self, fields: &mut Fields) {
                params::$kind::fields(self, default, fields);
            }
        })+

        #[derive(Clone, Copy, PartialEq, Debug)]
        pub enum Params {
            $($kind(params::$kind),)+
        }

        impl Params {
            pub fn default_for(kind: Kind) -> Params {
                match kind {
                    $(Kind::$kind => Params::$kind(params::$kind::DEFAULT),)+
                }
            }

            pub fn kind(&self) -> Kind {
                match self {
                    $(Params::$kind(_) => Kind::$kind,)+
                }
            }

            fn fields(&self, fields: &mut Fields) {
                match self {
                    $(Params::$kind(p) => p.fields(&params::$kind::DEFAULT, fields),)+
                }
            }

            pub fn automatable(&self, band: &str, name: &str) -> bool {
                match self {
                    $(Params::$kind(_) => face::param_of::<params::$kind>(params::$module::PARAMS, band, name).is_some(),)+
                }
            }

            pub fn travel_of(&self, band: &str, name: &str) -> Option<f64> {
                match self {
                    $(Params::$kind(p) => face::param_of::<params::$kind>(params::$module::PARAMS, band, name).map(|param| param.travel_of(p)),)+
                }
            }

            pub fn lane_share(&self, band: &str, name: &str) -> f64 {
                match self {
                    $(Params::$kind(_) => face::param_of::<params::$kind>(params::$module::PARAMS, band, name).map_or(1.0, |param| param.lane_share()),)+
                }
            }

            pub fn lane_seed(&self, band: &str, name: &str) -> Option<u64> {
                match self {
                    $(Params::$kind(_) => face::param_of::<params::$kind>(params::$module::PARAMS, band, name).map(|param| param.seed),)+
                }
            }

            pub fn set_travel(&mut self, band: &str, name: &str, travel: f64) -> bool {
                match self {
                    $(Params::$kind(p) => match face::param_of::<params::$kind>(params::$module::PARAMS, band, name) {
                        Some(param) => {
                            param.set_travel(p, travel);
                            true
                        }
                        None => false,
                    },)+
                }
            }

            fn from_json(kind: Kind, json: Option<&Json>) -> Params {
                match kind {
                    $(Kind::$kind => Params::$kind(params::$kind::from_json(json, params::$kind::DEFAULT)),)+
                }
            }
        }

        $(impl Patch for params::$kind {
            fn factory(&self) -> params::$kind {
                params::$kind::DEFAULT
            }
        })+

        $(impl face::Cells for params::$kind {
            const CELLS: &'static [sdk::face::Cell<Self>] = &face::$module::CELLS;
        })+

        pub enum Effect {
            $($kind(Box<$module::$kind>),)+
        }

        impl Effect {
            pub fn new(kind: Kind, sample_rate: f64, max_frames: usize) -> Effect {
                match kind {
                    $(Kind::$kind => Effect::$kind(Box::new(<$module::$kind as Unit>::new(sample_rate, max_frames))),)+
                }
            }

            pub fn restart(&mut self) {
                match self {
                    $(Effect::$kind(unit) => unit.restart(),)+
                }
            }

            pub fn holding(&self) -> bool {
                match self {
                    $(Effect::$kind(unit) => unit.holding(),)+
                }
            }

            pub fn feed(&mut self, l: &[f32], r: &[f32]) {
                match self {
                    $(Effect::$kind(unit) => unit.feed(l, r),)+
                }
            }

            pub fn starve(&mut self) {
                match self {
                    $(Effect::$kind(unit) => unit.starve(),)+
                }
            }

            pub fn process(&mut self, l: &mut [f32], r: &mut [f32], slot: &Slot, clock: &Clock) {
                let wet = slot.wet as f32;
                match (self, &slot.params) {
                    $((Effect::$kind(unit), Params::$kind(p)) => unit.process(l, r, p, wet, clock),)+
                    _ => {}
                }
            }

            pub fn gauge(&self) -> f32 {
                match self {
                    $(Effect::$kind(unit) => unit.gauge(),)+
                }
            }

            pub fn signal(&self) -> f32 {
                match self {
                    $(Effect::$kind(unit) => unit.signal(),)+
                }
            }

            pub fn strips(&self) -> [[f32; 2]; params::FREAK_BANDS] {
                match self {
                    $(Effect::$kind(unit) => unit.strips(),)+
                }
            }

            pub fn curve(&self) -> Option<&[f32; sdk::spectrum::CURVE]> {
                match self {
                    $(Effect::$kind(unit) => unit.curve(),)+
                }
            }
        }
    };
}

impl Kind {
    pub fn meters_reduction(self) -> bool {
        matches!(self, Kind::Compressor | Kind::Gate | Kind::Sidechain | Kind::Space | Kind::Suppressor)
    }

    pub fn takes_signal(self) -> bool {
        Params::default_for(self).signal().is_some()
    }
}

effects! {
    Compressor: compressor, "Compressor", "compressor";
    Delay: delay, "Delay", "delay";
    Eq: eq, "EQ", "eq";
    Freak: freak, "Freak", "freak";
    Gate: gate, "Gate", "gate";
    VolumeLfo: volume_lfo, "LFO", "volume-lfo";
    Motion: motion, "Motion", "motion";
    Repeat: repeat, "Repeat", "repeat";
    Reverb: reverb, "Reverb", "reverb";
    Sidechain: sidechain, "Sidechain", "sidechain";
    Space: space, "Space", "space";
    Suppressor: suppressor, "Suppressor", "suppressor";
    Time: time, "Time", "time";
    Transient: transient, "Transient", "transient";
    Utility: utility, "Utility", "utility";
}

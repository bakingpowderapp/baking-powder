use crate::json::Json;
use std::ops::RangeInclusive;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Scale {
    Linear { lo: f64, hi: f64, skew: f64 },
    Log { lo: f64, hi: f64 },
    Ratio,
    Range,
    Floored { lo: f64, hi: f64 },
    LimiterRelease,
}

impl Scale {
    pub fn travel(self, value: f64) -> f64 {
        match self {
            Scale::Linear { lo, hi, skew } => ((value - lo) / (hi - lo)).clamp(0.0, 1.0).powf(1.0 / skew),
            Scale::Log { lo, hi } => ((value.max(lo) / lo).ln() / (hi / lo).ln()).clamp(0.0, 1.0),
            Scale::Ratio => {
                let full = 1.0 - 1.0 / 20.0;
                ((1.0 - 1.0 / value.max(1.0001)) / full).clamp(0.0, 1.0).powf(1.0 / 1.5)
            }
            Scale::Range => {
                if value >= 60.0 {
                    1.0
                } else {
                    0.95 * Scale::Log { lo: 1.0, hi: 24.0 }.travel(value)
                }
            }
            Scale::Floored { lo, hi } => {
                if value <= 0.0 {
                    0.0
                } else {
                    0.05 + 0.95 * Scale::Log { lo, hi }.travel(value)
                }
            }
            Scale::LimiterRelease => 1.0 - limiter_release().travel(release_ms(value)),
        }
    }

    pub fn span(self) -> f64 {
        match self {
            Scale::Linear { lo, hi, .. } => hi - lo,
            Scale::Log { lo, hi } => (hi / lo).ln(),
            Scale::Ratio | Scale::Range | Scale::Floored { .. } | Scale::LimiterRelease => 1.0,
        }
    }

    pub fn value(self, travel: f64) -> f64 {
        let travel = travel.clamp(0.0, 1.0);
        crate::json::tidy(match self {
            Scale::Linear { lo, hi, skew } => lo + (hi - lo) * travel.powf(skew),
            Scale::Log { lo, hi } => lo * (hi / lo).powf(travel),
            Scale::Ratio => {
                let full = 1.0 - 1.0 / 20.0;
                1.0 / (1.0 - full * travel.powf(1.5)).max(0.0001)
            }
            Scale::Range => {
                if travel > 0.95 {
                    60.0
                } else {
                    Scale::Log { lo: 1.0, hi: 24.0 }.value(travel / 0.95)
                }
            }
            Scale::Floored { lo, hi } => {
                if travel < 0.05 {
                    0.0
                } else {
                    Scale::Log { lo, hi }.value((travel - 0.05) / 0.95)
                }
            }
            Scale::LimiterRelease => 6000.0 / limiter_release().value(1.0 - travel),
        })
    }
}

pub const LIMITER_RELEASE_MS: RangeInclusive<f64> = 1.0..=1000.0;

fn limiter_release() -> Scale {
    Scale::Log { lo: *LIMITER_RELEASE_MS.start(), hi: *LIMITER_RELEASE_MS.end() }
}

pub fn release_ms(db_per_second: f64) -> f64 {
    6000.0 / db_per_second.max(0.0001)
}

pub struct Field<P: 'static> {
    pub get: fn(&P) -> f64,
    pub set: fn(&mut P, f64),
}

#[macro_export]
macro_rules! field {
    ($get:expr, $set:expr) => {
        $crate::param::Field { get: $get, set: $set }
    };
    ($($path:tt)+) => {
        $crate::param::Field { get: |p| p.$($path)+, set: |p, v| p.$($path)+ = v }
    };
}

#[macro_export]
macro_rules! flag {
    ($($path:tt)+) => {
        $crate::param::Field { get: |p| if p.$($path)+ { 1.0 } else { 0.0 }, set: |p, v| p.$($path)+ = v >= 0.5 }
    };
}

#[macro_export]
macro_rules! int {
    ($($path:tt)+) => {
        $crate::param::Field { get: |p| p.$($path)+ as f64, set: |p, v| p.$($path)+ = v.round() as _ }
    };
}

#[macro_export]
macro_rules! choose {
    ($options:expr, $($path:tt)+) => {
        $crate::param::Field { get: |p| $crate::param::position($options, p.$($path)+), set: |p, v| p.$($path)+ = $crate::param::choice($options, v) }
    };
}

pub use crate::{choose, field, flag, int};

pub const UNIT: Scale = Scale::Linear { lo: 0.0, hi: 1.0, skew: 1.0 };

pub const BIPOLAR: Scale = Scale::Linear { lo: -1.0, hi: 1.0, skew: 1.0 };

pub const fn linear(lo: f64, hi: f64) -> Scale {
    Scale::Linear { lo, hi, skew: 1.0 }
}

pub const fn skewed(lo: f64, hi: f64, skew: f64) -> Scale {
    Scale::Linear { lo, hi, skew }
}

pub const fn log(lo: f64, hi: f64) -> Scale {
    Scale::Log { lo, hi }
}

pub const fn floored(lo: f64, hi: f64) -> Scale {
    Scale::Floored { lo, hi }
}

pub fn position<T: PartialEq>(options: &[T], current: T) -> f64 {
    options.iter().position(|option| *option == current).unwrap_or(0) as f64
}

pub fn choice<T: Copy>(options: &[T], value: f64) -> T {
    options[(value.round().max(0.0) as usize).min(options.len() - 1)]
}

pub struct Param<P: 'static> {
    pub band: &'static str,
    pub name: &'static str,
    pub seed: u64,
    pub path: Option<&'static str>,
    pub get: fn(&P) -> f64,
    pub set: fn(&mut P, f64),
    pub scale: Scale,
    pub lane: Scale,
    pub spell: fn(f64) -> f64,
    pub read: fn(f64) -> f64,
}

impl<P> Clone for Param<P> {
    fn clone(&self) -> Param<P> {
        *self
    }
}

impl<P> Copy for Param<P> {}

impl<P> Param<P> {
    pub const fn new(band: &'static str, name: &'static str, field: Field<P>, scale: Scale) -> Param<P> {
        Param { band, name, seed: seed(band, name), path: None, get: field.get, set: field.set, scale, lane: scale, spell: same, read: same }
    }

    pub const fn seeded(self, band: &str, name: &str) -> Param<P> {
        Param { seed: seed(band, name), ..self }
    }

    pub const fn saved(self, path: &'static str) -> Param<P> {
        Param { path: Some(path), ..self }
    }

    pub const fn keyed(self, spell: fn(f64) -> f64, read: fn(f64) -> f64) -> Param<P> {
        Param { spell, read, ..self }
    }

    pub const fn swung(self, lane: Scale) -> Param<P> {
        Param { lane, ..self }
    }

    pub fn lane_share(&self) -> f64 {
        if self.lane == self.scale { 1.0 } else { self.lane.span() / self.scale.span() }
    }

    pub fn travel_of(&self, p: &P) -> f64 {
        self.scale.travel((self.get)(p))
    }

    pub fn set_travel(&self, p: &mut P, travel: f64) {
        (self.set)(p, self.scale.value(travel));
    }
}

pub const fn seed(band: &str, name: &str) -> u64 {
    folded(folded(0, band.as_bytes()), name.as_bytes())
}

const fn folded(seed: u64, bytes: &[u8]) -> u64 {
    match bytes {
        [byte, rest @ ..] => folded(seed.wrapping_mul(31).wrapping_add(*byte as u64), rest),
        [] => seed,
    }
}

pub fn zero<P>(_: &P) -> f64 {
    0.0
}

pub fn same(value: f64) -> f64 {
    value
}

pub fn ignore<P>(_: &mut P, _: f64) {}

pub fn find<'a, P>(params: &'a [Param<P>], band: &str, name: &str) -> Option<&'a Param<P>> {
    params.iter().find(|param| param.band == band && param.name == name)
}

pub const fn flat<P, const N: usize>(parts: &[&[Param<P>]]) -> [Param<P>; N] {
    joined(parts, Param::new("", "", Field { get: zero, set: ignore }, UNIT))
}

pub(crate) const fn joined<T: Copy, const N: usize>(parts: &[&[T]], blank: T) -> [T; N] {
    let mut out = [blank; N];
    let mut k = 0;
    let mut i = 0;
    while i < parts.len() {
        let mut j = 0;
        while j < parts[i].len() {
            out[k] = parts[i][j];
            k += 1;
            j += 1;
        }
        i += 1;
    }
    assert!(k == N);
    out
}

pub trait Patch: Sized {
    fn factory(&self) -> Self;
}

pub struct Knob {
    pub path: String,
    pub band: &'static str,
    pub name: &'static str,
    pub scale: Scale,
    pub spell: fn(f64) -> f64,
    pub read: fn(f64) -> f64,
}

impl Knob {
    pub fn value(&self, travel: f64) -> f64 {
        (self.spell)(self.scale.value(travel))
    }

    pub fn travel(&self, value: f64) -> f64 {
        match [0.0, 1.0].into_iter().find(|end| self.value(*end) == value) {
            Some(end) => end,
            None => self.scale.travel((self.read)(value)),
        }
    }
}

const PROBES: [f64; 3] = [0.2, 0.5, 0.8];

pub fn lanes(knobs: &[Knob]) -> Json {
    let paths: Vec<&str> = knobs.iter().map(|knob| knob.path.as_str()).collect();
    Json::from(format!("the keys a curve or LFO lane may move, by path: {}", paths.join(", ")))
}

pub fn knobs<P: Copy>(params: &[Param<P>], default: P, spell: impl Fn(&P, &P) -> Json) -> Vec<Knob> {
    let turned = |param: &Param<P>, travel: f64| {
        let mut patch = default;
        param.set_travel(&mut patch, travel);
        patch
    };
    let probed = |param: &Param<P>| {
        let rest = param.travel_of(&default);
        let mut probes = PROBES;
        probes.sort_by(|one, other| (other - rest).abs().total_cmp(&(one - rest).abs()));
        let far = turned(param, probes[0]);
        probes[1..].iter().find_map(|&probe| {
            let near = turned(param, probe);
            spell(&far, &near).changed(&spell(&near, &far))
        })
    };
    let found = params.iter().filter_map(|param| {
        let path = param.path.map(String::from).or_else(|| probed(param))?;
        Some(Knob { path, band: param.band, name: param.name, scale: param.scale, spell: param.spell, read: param.read })
    });
    found.collect()
}

#[macro_export]
macro_rules! table {
    ($vis:vis $list:ident: $ty:ty { $($name:ident = $band:literal, $label:literal $(seeded $first_band:literal, $first_label:literal)?, $field:expr, $scale:expr $(=> $lane:expr)? $(, $spell:expr, $read:expr)?;)+ }) => {
        $(pub const $name: $crate::param::Param<$ty> = $crate::param::Param::<$ty>::new($band, $label, $field, $scale)$(.seeded($first_band, $first_label))?$(.swung($lane))?$(.keyed($spell, $read))?;)+
        $vis const $list: &[$crate::param::Param<$ty>] = &[$($name,)+];
    };
}

pub use crate::table;

#[macro_export]
macro_rules! bank {
    ($vis:vis $bank:ident: $ty:ty { $($field:ident),+ $(,)? }) => {
        #[derive(Clone, Copy)]
        $vis struct $bank {
            $(pub $field: $crate::param::Param<$ty>,)+
        }

        impl $bank {
            pub const COUNT: usize = [$(stringify!($field)),+].len();

            pub const fn all(&self) -> [$crate::param::Param<$ty>; Self::COUNT] {
                [$(self.$field,)+]
            }
        }
    };
    ($vis:vis $bank:ident<$p:ident> { $($field:ident),+ $(,)? }) => {
        $vis struct $bank<$p: 'static> {
            $(pub $field: $crate::param::Param<$p>,)+
        }

        impl<$p> Clone for $bank<$p> {
            fn clone(&self) -> Self {
                *self
            }
        }

        impl<$p> Copy for $bank<$p> {}

        impl<$p> $bank<$p> {
            pub const COUNT: usize = [$(stringify!($field)),+].len();

            pub const fn all(&self) -> [$crate::param::Param<$p>; [$(stringify!($field)),+].len()] {
                [$(self.$field,)+]
            }
        }
    };
}

pub use crate::bank;

use crate::json::Json;

pub const MAX_TIMES: usize = 4;

#[macro_export]
macro_rules! params {
    (@type number) => { f64 };
    (@type flag) => { bool };
    (@type choice $ty:ty) => { $ty };
    (@type custom $ty:ty) => { $ty };
    (@type nested $ty:ty) => { $ty };
    (@type always $ty:ty) => { $ty };
    (@type state $ty:ty) => { $ty };
    (@type switch) => { bool };
    (@type lead) => { f64 };
    (@type pick $ty:ty) => { $ty };
    (@type whole $ty:ty) => { $ty };
    (@type own $ty:ty) => { $ty };
    (@write whole $($ty:ty)?, $p:tt, $d:tt, $f:tt, $field:ident, $key:literal, $range:expr $(, $values:expr)?) => { $f.number($key, $p.$field as f64, $d.$field as f64) };
    (@write own $ty:ty, $p:tt, $d:tt, $f:tt, $field:ident, $key:literal, $write:expr, $read:expr, $manual:expr) => {{
        let write: fn(&Self, &Self, &str, &mut $crate::schema::Fields) = $write;
        write($p, $d, $key, $f)
    }};
    (@read whole $($ty:ty)?, $j:tt, $d:tt, $field:ident, $key:literal, $range:expr $(, $values:expr)?) => {{
        let range = $range;
        $crate::schema::number($j, $key, $d.$field as f64).round().clamp(*range.start() as f64, *range.end() as f64) as _
    }};
    (@read own $ty:ty, $j:tt, $d:tt, $field:ident, $key:literal, $write:expr, $read:expr, $manual:expr) => {{
        let read: fn(Option<&$crate::json::Json>, &Self, &str) -> $ty = $read;
        read($j, &$d, $key)
    }};
    (@manual whole $($ty:ty)?, $f:tt, $d:tt, $describe:tt, $prefix:tt, $field:ident, $key:literal, $range:expr) => {{
        let range = $range;
        $f.manual($key, $crate::schema::bounded($d.$field as f64, *range.start() as f64..=*range.end() as f64, true), &$describe(&$crate::schema::path($prefix, $key)))
    }};
    (@manual whole $($ty:ty)?, $f:tt, $d:tt, $describe:tt, $prefix:tt, $field:ident, $key:literal, $range:expr, $values:expr) => { $f.manual($key, $crate::schema::worded($values, $d.$field as f64), &$describe(&$crate::schema::path($prefix, $key))) };
    (@manual own $ty:ty, $f:tt, $d:tt, $describe:tt, $prefix:tt, $field:ident, $key:literal, $write:expr, $read:expr, $manual:expr) => {{
        let manual: fn(&Self, &str, &str, &mut $crate::schema::Fields) = $manual;
        manual($d, $key, &$describe(&$crate::schema::path($prefix, $key)), $f)
    }};
    (@write number, $p:tt, $d:tt, $f:tt, $field:ident, $key:literal, $range:expr) => { $f.number($key, $p.$field, $d.$field) };
    (@write flag, $p:tt, $d:tt, $f:tt, $field:ident, $key:literal) => { $f.flag($key, $p.$field, $d.$field) };
    (@write choice $ty:ty, $p:tt, $d:tt, $f:tt, $field:ident, $key:literal) => { $f.name($key, $p.$field.name(), $d.$field.name()) };
    (@write custom $ty:ty, $p:tt, $d:tt, $f:tt, $field:ident, $key:literal, $manual:expr, $write:expr, $read:expr) => {{
        let write: fn(&Self, &Self, &mut $crate::schema::Fields) = $write;
        write($p, $d, $f)
    }};
    (@write nested $ty:ty, $p:tt, $d:tt, $f:tt, $field:ident, $key:literal) => { $f.nested($key, |fields| $p.$field.fields(&$d.$field, fields)) };
    (@write always $ty:ty, $p:tt, $d:tt, $f:tt, $field:ident, $key:literal) => { $f.whole($key, |fields| $p.$field.fields(&$d.$field, fields)) };
    (@write state $ty:ty, $p:tt, $d:tt, $f:tt, $field:ident) => {};
    (@write switch, $p:tt, $d:tt, $f:tt, $field:ident, $key:literal) => { $f.lead($key, $crate::json::Json::Bool($p.$field), $p.$field == $d.$field) };
    (@write lead, $p:tt, $d:tt, $f:tt, $field:ident, $key:literal, $range:expr) => { $f.lead($key, $crate::json::Json::Number($p.$field), $p.$field == $d.$field) };
    (@write pick $ty:ty, $p:tt, $d:tt, $f:tt, $field:ident, $key:literal) => { $f.lead($key, $crate::json::Json::String(String::from($p.$field.name())), $p.$field == $d.$field) };
    (@read number, $j:tt, $d:tt, $field:ident, $key:literal, $range:expr) => {{
        let range = $range;
        $crate::schema::number($j, $key, $d.$field).clamp(*range.start(), *range.end())
    }};
    (@read flag, $j:tt, $d:tt, $field:ident, $key:literal) => { $crate::schema::flag($j, $key, $d.$field) };
    (@read choice $ty:ty, $j:tt, $d:tt, $field:ident, $key:literal) => { $crate::schema::name($j, $key).and_then(<$ty>::from_name).unwrap_or($d.$field) };
    (@read custom $ty:ty, $j:tt, $d:tt, $field:ident, $key:literal, $manual:expr, $write:expr, $read:expr) => {{
        let read: fn(Option<&$crate::json::Json>, &Self) -> $ty = $read;
        read($j, &$d)
    }};
    (@read nested $ty:ty, $j:tt, $d:tt, $field:ident, $key:literal) => { <$ty>::from_json($j.and_then(|j| j.get($key)), $d.$field) };
    (@read always $ty:ty, $j:tt, $d:tt, $field:ident, $key:literal) => { <$ty>::from_json($j.and_then(|j| j.get($key)), $d.$field) };
    (@read state $ty:ty, $j:tt, $d:tt, $field:ident) => { $d.$field };
    (@read switch, $j:tt, $d:tt, $field:ident, $key:literal) => { $crate::schema::flag($j, $key, $d.$field) };
    (@read lead, $j:tt, $d:tt, $field:ident, $key:literal, $range:expr) => {{
        let range = $range;
        $crate::schema::number($j, $key, $d.$field).clamp(*range.start(), *range.end())
    }};
    (@read pick $ty:ty, $j:tt, $d:tt, $field:ident, $key:literal) => { $crate::schema::name($j, $key).and_then(<$ty>::from_name).unwrap_or($d.$field) };
    (@manual number, $f:tt, $d:tt, $describe:tt, $prefix:tt, $field:ident, $key:literal, $range:expr) => { $f.manual($key, $crate::schema::bounded($d.$field, $range, false), &$describe(&$crate::schema::path($prefix, $key))) };
    (@manual flag, $f:tt, $d:tt, $describe:tt, $prefix:tt, $field:ident, $key:literal) => { $f.manual($key, $crate::schema::either($d.$field), &$describe(&$crate::schema::path($prefix, $key))) };
    (@manual choice $ty:ty, $f:tt, $d:tt, $describe:tt, $prefix:tt, $field:ident, $key:literal) => { $f.manual($key, $crate::schema::one_of(<$ty>::KEYS, $d.$field.name()), &$describe(&$crate::schema::path($prefix, $key))) };
    (@manual custom $ty:ty, $f:tt, $d:tt, $describe:tt, $prefix:tt, $field:ident, $key:literal, $manual:expr, $write:expr, $read:expr) => { $f.manual($key, $crate::json::Json::from($manual), &$describe(&$crate::schema::path($prefix, $key))) };
    (@manual nested $ty:ty, $f:tt, $d:tt, $describe:tt, $prefix:tt, $field:ident, $key:literal) => { $f.whole($key, |fields| <$ty>::manual(&$d.$field, &$crate::schema::inner($prefix, $key), $describe, fields)) };
    (@manual always $ty:ty, $f:tt, $d:tt, $describe:tt, $prefix:tt, $field:ident, $key:literal) => { $f.whole($key, |fields| <$ty>::manual(&$d.$field, &$crate::schema::inner($prefix, $key), $describe, fields)) };
    (@manual state $ty:ty, $f:tt, $d:tt, $describe:tt, $prefix:tt, $field:ident) => {};
    (@manual switch, $f:tt, $d:tt, $describe:tt, $prefix:tt, $field:ident, $key:literal) => { $f.manual($key, $crate::schema::either($d.$field), &$describe(&$crate::schema::path($prefix, $key))) };
    (@manual lead, $f:tt, $d:tt, $describe:tt, $prefix:tt, $field:ident, $key:literal, $range:expr) => { $f.manual($key, $crate::schema::bounded($d.$field, $range, false), &$describe(&$crate::schema::path($prefix, $key))) };
    (@manual pick $ty:ty, $f:tt, $d:tt, $describe:tt, $prefix:tt, $field:ident, $key:literal) => { $f.manual($key, $crate::schema::one_of(<$ty>::KEYS, $d.$field.name()), &$describe(&$crate::schema::path($prefix, $key))) };
    (@write number, $p:tt, $d:tt, $f:tt, $field:ident, $key:literal, $range:expr, $values:expr) => { $crate::params!(@write number, $p, $d, $f, $field, $key, $range) };
    (@read number, $j:tt, $d:tt, $field:ident, $key:literal, $range:expr, $values:expr) => { $crate::params!(@read number, $j, $d, $field, $key, $range) };
    (@manual number, $f:tt, $d:tt, $describe:tt, $prefix:tt, $field:ident, $key:literal, $range:expr, $values:expr) => { $f.manual($key, $crate::schema::worded($values, $d.$field), &$describe(&$crate::schema::path($prefix, $key))) };
    (@write single, $p:tt, $d:tt, $f:tt, $field:ident, $key:literal, $range:expr) => { $f.number($key, $crate::json::single($p.$field), $crate::json::single($d.$field)) };
    (@read single, $j:tt, $d:tt, $field:ident, $key:literal, $range:expr) => {{
        let range = $range;
        ($crate::schema::number($j, $key, f64::from($d.$field)) as f32).clamp(*range.start(), *range.end())
    }};
    (@manual single, $f:tt, $d:tt, $describe:tt, $prefix:tt, $field:ident, $key:literal, $range:expr) => {{
        let range = $range;
        $f.manual($key, $crate::schema::bounded($crate::json::single($d.$field), $crate::json::single(*range.start())..=$crate::json::single(*range.end()), false), &$describe(&$crate::schema::path($prefix, $key)))
    }};
    (@write text, $p:tt, $d:tt, $f:tt, $field:ident, $key:literal) => { $f.name($key, &$p.$field, &$d.$field) };
    (@read text, $j:tt, $d:tt, $field:ident, $key:literal) => { $crate::schema::name($j, $key).map_or_else(|| $d.$field.clone(), String::from) };
    (@manual text, $f:tt, $d:tt, $describe:tt, $prefix:tt, $field:ident, $key:literal) => { $f.manual($key, $crate::schema::text(&$d.$field), &$describe(&$crate::schema::path($prefix, $key))) };
    ($name:ident { $($kind:ident $field:ident $(: $ty:ty)? = $default:expr $(, $key:literal $(, $arg:expr)*)?;)+ }) => {
        #[derive(Clone, Copy, PartialEq, Debug)]
        pub struct $name {
            $(pub $field: $crate::params!(@type $kind $($ty)?),)+
        }

        impl $name {
            pub const DEFAULT: $name = $name { $($field: $default,)+ };

            pub fn fields(&self, default: &$name, fields: &mut $crate::schema::Fields) {
                $($crate::params!(@write $kind $($ty)?, self, default, fields, $field $(, $key $(, $arg)*)?);)+
            }

            pub fn from_json(json: Option<&$crate::json::Json>, default: $name) -> $name {
                $name { $($field: $crate::params!(@read $kind $($ty)?, json, default, $field $(, $key $(, $arg)*)?),)+ }
            }

            pub fn manual(default: &$name, prefix: &str, describe: &dyn Fn(&str) -> String, fields: &mut $crate::schema::Fields) {
                $($crate::params!(@manual $kind $($ty)?, fields, default, describe, prefix, $field $(, $key $(, $arg)*)?);)+
            }
        }
    };
    ($name:ident ($fields:ident, $from_json:ident, $manual:ident) { $($kind:ident $field:ident $(: $ty:ty)? = $default:expr $(, $key:literal $(, $arg:expr)*)? $(=> $about:literal)?;)+ }) => {
        #[derive(Clone, Copy, PartialEq, Debug)]
        pub struct $name {
            $(pub $field: $crate::params!(@type $kind $($ty)?),)+
        }

        impl $name {
            pub const DEFAULT: $name = $name { $($field: $default,)+ };

            pub fn $fields(&self, default: &$name, fields: &mut $crate::schema::Fields) {
                $($crate::params!(@write $kind $($ty)?, self, default, fields, $field $(, $key $(, $arg)*)?);)+
            }

            pub fn $from_json(json: Option<&$crate::json::Json>, default: $name) -> $name {
                $name { $($field: $crate::params!(@read $kind $($ty)?, json, default, $field $(, $key $(, $arg)*)?),)+ }
            }

            pub fn $manual(default: &$name, fields: &mut $crate::schema::Fields) {
                $($crate::params!(@manual $kind $($ty)?, fields, default, (&|_: &str| String::from(concat!("" $(, $about)?))), "", $field $(, $key $(, $arg)*)?);)+
            }
        }
    };
    ($vis:vis impl $name:ident ($write:ident, $read:ident, $manual:ident) { $($kind:ident $field:ident $(: $ty:ty)?, $key:literal $(, $arg:expr)* $(=> $about:literal)?;)+ }) => {
        impl $name {
            $vis fn $write(&self, default: &$name, fields: &mut $crate::schema::Fields) {
                $($crate::params!(@write $kind $($ty)?, self, default, fields, $field, $key $(, $arg)*);)+
            }

            $vis fn $read(&mut self, json: Option<&$crate::json::Json>) {
                $(self.$field = $crate::params!(@read $kind $($ty)?, json, self, $field, $key $(, $arg)*);)+
            }

            $vis fn $manual(default: &$name, fields: &mut $crate::schema::Fields) {
                $($crate::params!(@manual $kind $($ty)?, fields, default, (&|_: &str| String::from(concat!("" $(, $about)?))), "", $field, $key $(, $arg)*);)+
            }
        }
    };
}

pub use crate::params;

#[macro_export]
macro_rules! choice {
    ($vis:vis $name:ident { $($variant:ident = $label:literal / $key:literal),+ $(,)? }) => {
        $crate::choice!($vis $name { $($variant = $key),+ });

        impl $name {
            pub const LABELS: &'static [&'static str] = &[$($label),+];
        }
    };
    ($vis:vis $name:ident { $($variant:ident = $key:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
        $vis enum $name {
            #[default]
            $($variant),+
        }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];
            pub const KEYS: &'static [&'static str] = &[$($key),+];

            pub fn index(self) -> usize {
                $name::ALL.iter().position(|choice| *choice == self).unwrap_or(0)
            }

            pub fn name(self) -> &'static str {
                $name::KEYS[self.index()]
            }

            pub fn from_name(name: &str) -> Option<$name> {
                $name::KEYS.iter().position(|key| *key == name).map(|index| $name::ALL[index])
            }
        }
    };
}

pub use crate::choice;

#[derive(Default)]
pub struct Fields {
    entries: Vec<(String, Json)>,
    leads: usize,
}

impl Fields {
    pub fn new() -> Fields {
        Fields { entries: Vec::new(), leads: 0 }
    }

    pub fn push(&mut self, key: &str, value: Json) {
        self.entries.push((String::from(key), value));
    }

    pub fn lead(&mut self, key: &str, value: Json, at_default: bool) {
        self.push(key, value);
        if at_default {
            self.leads += 1;
        }
    }

    pub fn number(&mut self, key: &str, value: f64, default: f64) {
        if value != default {
            self.push(key, Json::Number(value));
        }
    }

    pub fn flag(&mut self, key: &str, value: bool, default: bool) {
        if value != default {
            self.push(key, Json::Bool(value));
        }
    }

    pub fn name(&mut self, key: &str, value: &str, default: &str) {
        if value != default {
            self.push(key, Json::String(String::from(value)));
        }
    }

    pub fn object(&mut self, key: &str, fields: Fields) {
        if let Some(object) = fields.into_object() {
            self.push(key, object);
        }
    }

    pub fn nested(&mut self, key: &str, write: impl FnOnce(&mut Fields)) {
        let mut fields = Fields::new();
        write(&mut fields);
        self.object(key, fields);
    }

    pub fn whole(&mut self, key: &str, write: impl FnOnce(&mut Fields)) {
        let mut fields = Fields::new();
        write(&mut fields);
        self.push(key, fields.into_json());
    }

    pub fn manual(&mut self, key: &str, what: Json, about: &str) {
        match what {
            Json::String(text) if text.is_empty() => {}
            Json::String(text) if about.is_empty() => self.push(key, Json::String(text)),
            Json::String(text) => self.push(key, Json::String(format!("{text}: {}", lowered(about)))),
            shape => self.push(key, shape),
        }
    }

    pub fn list(&mut self, key: &str, items: Vec<Json>) {
        if !items.is_empty() {
            self.push(key, Json::Array(items));
        }
    }

    pub fn into_object(self) -> Option<Json> {
        if self.entries.len() == self.leads { None } else { Some(Json::Object(self.entries)) }
    }

    pub fn into_json(self) -> Json {
        Json::Object(self.entries)
    }
}

pub fn number(json: Option<&Json>, key: &str, default: f64) -> f64 {
    json.and_then(|j| j.get(key)).and_then(Json::as_f64).filter(|n| n.is_finite()).unwrap_or(default)
}

pub fn flag(json: Option<&Json>, key: &str, default: bool) -> bool {
    json.and_then(|j| j.get(key)).and_then(Json::as_bool).unwrap_or(default)
}

pub fn index(json: Option<&Json>, key: &str, limit: usize) -> Option<usize> {
    json.and_then(|j| j.get(key)).and_then(|value| value.as_index(limit))
}

pub fn name<'a>(json: Option<&'a Json>, key: &str) -> Option<&'a str> {
    json.and_then(|j| j.get(key)).and_then(Json::as_str)
}

pub fn items<'a>(json: Option<&'a Json>, key: &str) -> &'a [Json] {
    json.and_then(|j| j.get(key)).and_then(Json::as_array).unwrap_or(&[])
}

fn lowered(about: &str) -> String {
    let plain = about.split(' ').next().is_some_and(|word| word.chars().skip(1).all(|c| !c.is_uppercase()));
    match about.chars().next() {
        Some(first) if first.is_uppercase() && plain => first.to_lowercase().chain(about.chars().skip(1)).collect(),
        _ => String::from(about),
    }
}

pub fn bounded(default: f64, range: std::ops::RangeInclusive<f64>, whole: bool) -> Json {
    let kind = if whole { "whole number" } else { "number" };
    Json::String(format!("{kind} {} to {}, default {}", crate::json::number(*range.start()), crate::json::number(*range.end()), crate::json::number(default)))
}

pub fn either(default: bool) -> Json {
    Json::String(format!("true or false, default {default}"))
}

pub fn one_of(names: &[&str], default: &str) -> Json {
    let typed = if names.iter().any(|name| name.parse::<f64>().is_ok()) { TEXT } else { "" };
    Json::String(format!("{typed}one of {}; default {default}", names.join(", ")))
}

pub fn worded(values: &str, default: f64) -> Json {
    Json::String(format!("{values}, default {}", crate::json::number(default)))
}

pub const TEXT: &str = "text, ";
pub const EMPTY: &str = "empty";

pub fn text(default: &str) -> Json {
    Json::String(format!("{TEXT}default {}", if default.is_empty() { EMPTY } else { default }))
}

pub fn path(prefix: &str, key: &str) -> String {
    format!("{prefix}{key}")
}

pub fn inner(prefix: &str, key: &str) -> String {
    format!("{prefix}{key}.")
}

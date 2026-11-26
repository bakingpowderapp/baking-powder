use super::*;
use Format::*;
use Kind::*;

const PERCENT: &[(&str, f64)] = &[("%", 1.0)];
const HERTZ: &[(&str, f64)] = &[("khz", 1000.0), ("hz", 1.0), ("k", 1000.0)];
const MILLIS: &[(&str, f64)] = &[("ms", 1.0), ("s", 1000.0)];
const SECONDS: &[(&str, f64)] = &[("ms", 0.001), ("s", 1.0)];
const DECIBELS: &[(&str, f64)] = &[("db", 1.0)];
const DEGREES: &[(&str, f64)] = &[("°", 1.0), ("deg", 1.0)];
const SHUT_DB: f64 = 80.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Percent,
    SignedPercent,
    Semitones,
    Cents,
    SignedCents,
    CentsTenth,
    Integer,
    SignedInteger,
    Hertz,
    LfoHertz,
    HertzOff,
    HertzOffAt20,
    HertzOffAt20k,
    Millis,
    MillisWhole,
    MillisTenth,
    MillisHundredths,
    MillisOffBelowHalf,
    SignedMillis,
    Seconds,
    SecondsOff,
    TwoPlaces,
    Tenths,
    Shelf,
    GlideTime,
    Decibels,
    DecibelsUnsigned,
    DecibelsPerSecond,
    Floor,
    Degrees,
    Spread,
    Times,
    Ratio,
    Range,
    Sixteenths,
    Octaves,
    Balance,
    Flat,
    Release,
}

pub fn spell(format: Format, value: f64) -> String {
    match format {
        Percent => format!("{:.0}%", value * 100.0),
        SignedPercent => format!("{:+.0}%", value * 100.0),
        Semitones => format!("{:+} st", value.round() as i64),
        Cents => format!("{value:.0}¢"),
        SignedCents => format!("{:+}¢", value.round() as i64),
        CentsTenth => format!("{value:.1}¢"),
        Integer => format!("{}", value.round() as i64),
        SignedInteger => format!("{:+}", value.round() as i64),
        Hertz => hertz(value),
        LfoHertz => {
            if value < 10.0 {
                format!("{value:.2} Hz")
            } else {
                format!("{value:.1} Hz")
            }
        }
        HertzOff => off_or(value < 0.5, || hertz(value)),
        HertzOffAt20 => off_or(value <= 20.0, || hertz(value)),
        HertzOffAt20k => off_or(value >= 20_000.0, || hertz(value)),
        Millis => millis(value),
        MillisWhole => format!("{} ms", value.round() as i64),
        MillisTenth => format!("{value:.1} ms"),
        MillisHundredths => format!("{value:.2} ms"),
        MillisOffBelowHalf => off_or(value < 0.5, || format!("{} ms", value.round() as i64)),
        SignedMillis => match value.round() as i64 {
            0 => String::from("0 ms"),
            ms => format!("{ms:+} ms"),
        },
        Seconds => {
            if value >= 1.0 {
                format!("{value:.2} s")
            } else {
                format!("{} ms", (value * 1000.0).round() as i64)
            }
        }
        SecondsOff => off_or(value < 0.0005, || spell(Seconds, value)),
        TwoPlaces => format!("{value:.2}"),
        Tenths => format!("{value:.1}"),
        Shelf => shelf(value),
        GlideTime => glide_time(value),
        Decibels => format!("{value:+.1} dB"),
        DecibelsUnsigned => format!("{value:.1} dB"),
        DecibelsPerSecond => format!("{} dB/s", value.round() as i64),
        Floor => {
            if value <= -SHUT_DB {
                String::from("-inf dB")
            } else {
                format!("{value:+.1} dB")
            }
        }
        Degrees => format!("{}°", (value * 360.0).round() as i64 % 360),
        Spread => format!("{:.0}°", value * 180.0),
        Times => format!("×{value:.2}"),
        Ratio => ratio(value),
        Range => off_or(value >= 60.0, || format!("{value:.1} dB")),
        Sixteenths => format!("{}/16", value.round() as i64),
        Octaves => format!("{value:.2} oct"),
        Balance => balance(value),
        Flat => {
            if value == 0.0 {
                String::from("Flat")
            } else {
                format!("{:+.0}", value * 100.0)
            }
        }
        Release => millis(release_ms(value)),
    }
}

fn off_or(off: bool, text: impl FnOnce() -> String) -> String {
    if off { String::from("off") } else { text() }
}

fn hertz(hz: f64) -> String {
    if hz < 1000.0 {
        format!("{} Hz", hz.round() as i64)
    } else if hz >= 10_000.0 {
        format!("{:.1} kHz", hz / 1000.0)
    } else {
        format!("{:.2} kHz", hz / 1000.0)
    }
}

fn millis(ms: f64) -> String {
    if ms < 0.995 {
        format!("{ms:.2} ms")
    } else if ms < 9.95 {
        format!("{ms:.1} ms")
    } else if ms < 999.5 {
        format!("{ms:.0} ms")
    } else {
        format!("{:.2} s", ms / 1000.0)
    }
}

fn shelf(tone: f64) -> String {
    if tone.abs() < 0.005 {
        String::from("open")
    } else if tone < 0.0 {
        format!("HP {:.0}%", -tone * 100.0)
    } else {
        format!("LP {:.0}%", tone * 100.0)
    }
}

fn glide_time(ms: f64) -> String {
    if ms < 0.5 {
        String::from("off")
    } else if ms < 1000.0 {
        format!("{} ms", ms.round() as i64)
    } else {
        format!("{:.2} s", ms / 1000.0)
    }
}

fn ratio(r: f64) -> String {
    if r < 1.05 {
        String::from("1:1")
    } else if r < 9.95 {
        format!("{r:.1}:1")
    } else {
        format!("{r:.0}:1")
    }
}

fn balance(v: f64) -> String {
    let percent = (v.abs() * 100.0).round() as i64;
    if percent == 0 {
        String::from("C")
    } else if v < 0.0 {
        format!("{percent}L")
    } else {
        format!("{percent}R")
    }
}

pub fn read(format: Format, text: &str) -> Option<f64> {
    let text = squeezed(text);
    let text = text.as_str();
    match format {
        Percent | SignedPercent => amount(text, PERCENT).map(|n| n / 100.0),
        Semitones => amount(text, &[("st", 1.0)]),
        Cents | SignedCents | CentsTenth => amount(text, &[("¢", 1.0), ("c", 1.0)]),
        Integer | SignedInteger | TwoPlaces | Tenths => amount(text, &[]),
        Hertz | LfoHertz => amount(text, HERTZ),
        HertzOff | HertzOffAt20 => named(text, "off", 0.0).or_else(|| amount(text, HERTZ)),
        HertzOffAt20k => named(text, "off", 20_000.0).or_else(|| amount(text, HERTZ)),
        Millis | MillisWhole | MillisTenth | MillisHundredths | SignedMillis => amount(text, MILLIS),
        MillisOffBelowHalf | GlideTime => named(text, "off", 0.0).or_else(|| amount(text, MILLIS)),
        Seconds => amount(text, SECONDS),
        SecondsOff => named(text, "off", 0.0).or_else(|| amount(text, SECONDS)),
        Shelf => named(text, "open", 0.0).or_else(|| sided(text, "hp", "lp").map(|n| n / 100.0)),
        Decibels | DecibelsUnsigned => amount(text, DECIBELS),
        DecibelsPerSecond => amount(text, &[("db/s", 1.0), ("db", 1.0)]),
        Floor => named(text, "-inf", -SHUT_DB).or_else(|| amount(text, DECIBELS)),
        Range => named(text, "off", 60.0).or_else(|| amount(text, DECIBELS)),
        Degrees => amount(text, DEGREES).map(|degrees| degrees / 360.0),
        Spread => amount(text, DEGREES).map(|degrees| degrees / 180.0),
        Times => amount(text.trim_start_matches(['x', '×']), &[("x", 1.0), ("×", 1.0)]),
        Ratio => amount(text, &[(":1", 1.0)]),
        Sixteenths => amount(text, &[("/16", 1.0)]),
        Octaves => amount(text, &[("oct", 1.0)]),
        Balance => named(text, "c", 0.0).or_else(|| ended(text, 'l', 'r').map(|n| n / 100.0)),
        Flat => named(text, "flat", 0.0).or_else(|| amount(text, PERCENT).map(|n| n / 100.0)),
        Release => amount(text, MILLIS).filter(|ms| *ms > 0.0).map(|ms| 6000.0 / ms),
    }
}

pub fn suffix(format: Format) -> &'static str {
    match format {
        Percent | SignedPercent | Flat => "%",
        Semitones => " st",
        Cents | SignedCents | CentsTenth => "¢",
        Hertz | LfoHertz | HertzOff | HertzOffAt20 | HertzOffAt20k => " Hz",
        Millis | MillisWhole | MillisTenth | MillisHundredths | MillisOffBelowHalf | SignedMillis | GlideTime | Release => " ms",
        Seconds | SecondsOff => " s",
        DecibelsPerSecond => " dB/s",
        Decibels | DecibelsUnsigned | Floor | Range => " dB",
        Degrees | Spread => "°",
        Times => "×",
        Ratio => ":1",
        Sixteenths => "/16",
        Octaves => " oct",
        Integer | SignedInteger | TwoPlaces | Tenths | Shelf | Balance => "",
    }
}

pub fn starts_value(c: char) -> bool {
    c.is_ascii_digit() || matches!(c, '-' | '+' | '.')
}

pub fn option_of(options: &[&str], text: &str) -> Option<usize> {
    let wanted = squeezed(text);
    if wanted.is_empty() {
        return None;
    }
    options.iter().position(|option| squeezed(option) == wanted).or_else(|| options.iter().position(|option| squeezed(option).starts_with(&wanted)))
}

pub(super) fn ghost<P>(cell: &Cell<P>) -> &'static str {
    match cell.kind {
        Rise | Centre | Pan => suffix(cell.format),
        Toggle | Switch | Pick | Button(_) | Page(_) | Open(..) | Title => "",

    }
}

pub(super) fn room<P>(cell: &Cell<P>) -> usize {
    (ENTRY_GLYPHS - 1).saturating_sub(ghost(cell).chars().count())
}

fn squeezed(text: &str) -> String {

    text.chars().filter(|c| !c.is_whitespace()).map(|c| c.to_ascii_lowercase()).collect()
}

fn amount(text: &str, units: &[(&str, f64)]) -> Option<f64> {
    let (digits, factor) = units.iter().find_map(|(unit, factor)| text.strip_suffix(unit).map(|digits| (digits, *factor))).unwrap_or((text, 1.0));
    digits.parse::<f64>().ok().filter(|amount| amount.is_finite()).map(|amount| amount * factor)
}

fn named(text: &str, word: &str, value: f64) -> Option<f64> {
    (text == word).then_some(value)
}

fn sided(text: &str, low: &str, high: &str) -> Option<f64> {
    match (text.strip_prefix(low), text.strip_prefix(high)) {
        (Some(rest), _) => amount(rest, PERCENT).map(|n| -n),
        (None, Some(rest)) => amount(rest, PERCENT),
        (None, None) => amount(text, PERCENT),
    }
}

fn ended(text: &str, low: char, high: char) -> Option<f64> {
    match (text.strip_suffix(low), text.strip_suffix(high)) {
        (Some(rest), _) => amount(rest, &[]).map(|n| -n),
        (None, Some(rest)) => amount(rest, &[]),
        (None, None) => amount(text, PERCENT),
    }
}

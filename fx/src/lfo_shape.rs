use sdk::division::Division;
use sdk::face::Drawn;
use crate::Clock;
use crate::params::{RATE_MAX_HZ, RATE_MIN_HZ};
use sdk::json::Json;
use sdk::schema::{Fields, bounded, either, flag, items, name, number, one_of, path};

pub(crate) fn seconds(clock: &Clock) -> f64 {
    clock.beat.map_or(0.0, |beat| beat.at(0) * beat.frames_per_beat / clock.sample_rate)
}

pub(crate) fn turns(clock: &Clock, sync: bool, division: Division, rate_hz: f64) -> f64 {
    match clock.beat {
        Some(beat) if sync && division.hz(clock.bpm) <= RATE_MAX_HZ => beat.at(0) / division.beats(),
        _ => seconds(clock) * clock.rate(sync, division, rate_hz),
    }
}

pub(crate) fn begun(start: f64, turns: f64) -> (i64, f64) {
    if turns == 0.0 {
        return (0, start);
    }
    let reached = start + turns;
    (reached.floor() as i64, reached - reached.floor())
}

const JOIN_SECONDS: f64 = 0.020;

#[derive(Clone, Copy, Default)]
pub(crate) struct Join {
    apart: f64,
    left: usize,
    span: usize,
}

impl Join {
    pub(crate) fn begin(&mut self, was: f64, now: f64, sample_rate: f64) {
        let apart = was + self.lag() - now;
        self.apart = apart - apart.round();
        self.span = (JOIN_SECONDS * sample_rate) as usize;
        self.left = self.span;
    }

    fn lag(&self) -> f64 {
        if self.left == 0 {
            return 0.0;
        }
        let u = self.left as f64 / self.span as f64;
        self.apart * u * u * (3.0 - 2.0 * u)
    }

    pub(crate) fn swept(&mut self, phase: f64) -> f64 {
        if self.left == 0 {
            return phase;
        }
        let eased = phase + self.lag();
        self.left -= 1;
        eased - eased.floor()
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct VolumeLfo {
    pub sync: bool,
    pub division: Division,
    pub rate_hz: f64,
    pub phase: f64,
    pub drawn: Drawn,
}

impl VolumeLfo {
    pub const DEFAULT: VolumeLfo = VolumeLfo { sync: true, division: Division::Quarter, rate_hz: 1.0, phase: 0.0, drawn: Drawn::RAMP };

    pub fn value_at(&self, phase: f64) -> f64 {
        self.drawn.value_at(phase)
    }

    pub(super) fn fields(&self, d: &VolumeLfo, fields: &mut Fields) {
        fields.lead("sync", Json::Bool(self.sync), self.sync == d.sync);
        fields.lead("division", Json::String(String::from(self.division.name())), self.division == d.division);
        fields.lead("rateHz", Json::Number(self.rate_hz), self.rate_hz == d.rate_hz);
        fields.number("startPhase", self.phase, d.phase);
        fields.lead("points", self.drawn.to_json(), self.drawn == d.drawn);
    }

    pub(super) fn manual(default: &VolumeLfo, prefix: &str, describe: &dyn Fn(&str) -> String, fields: &mut Fields) {
        fields.manual("sync", either(default.sync), &describe(&path(prefix, "sync")));
        fields.manual("division", one_of(Division::KEYS, default.division.name()), &describe(&path(prefix, "division")));
        fields.manual("rateHz", bounded(default.rate_hz, RATE_MIN_HZ..=RATE_MAX_HZ, false), &describe(&path(prefix, "rateHz")));
        fields.manual("startPhase", bounded(default.phase, 0.0..=1.0, false), &describe(&path(prefix, "startPhase")));
        fields.push("points", Drawn::manual("2 to 32 points in order of x, the shape of one cycle; the first sits at x 0 and the last at x 1; default a ramp from 0 to 1"));
    }

    pub(super) fn from_json(json: Option<&Json>, d: VolumeLfo) -> VolumeLfo {
        VolumeLfo {
            sync: flag(json, "sync", d.sync),
            division: name(json, "division").and_then(Division::from_name).unwrap_or(d.division),
            rate_hz: number(json, "rateHz", d.rate_hz).clamp(RATE_MIN_HZ, RATE_MAX_HZ),
            phase: number(json, "startPhase", d.phase).clamp(0.0, 1.0),
            drawn: Drawn::from_json(items(json, "points")).unwrap_or(d.drawn),
        }
    }
}

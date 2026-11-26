use super::{GAIN, MIX, SEND, SEND_NAMES, SongNote, Track, VOL, beats};
use crate::arp::{Join, MidiRow};
use sdk::division::Division;
use sdk::dsp::{sample_hold, square, triangle};
use sdk::face::Drawn;
use sdk::param;
use sdk::schema::choice;
use std::f64::consts::{PI, TAU};

choice!(pub LfoClock { Song = "song", Note = "note" });

choice!(pub LfoShape { Sine = "Sine" / "sine", Triangle = "Triangle" / "triangle", Saw = "Saw" / "saw", Ramp = "Ramp" / "ramp", Square = "Square" / "square", Random = "Random" / "random", Walk = "Walk" / "walk", Drawn = "Drawn" / "drawn" });

choice!(pub LfoField { Amount = "AMOUNT" / "amount", Phase = "PHASE" / "startPhase", Center = "CENTER" / "center" });

impl LfoField {
    pub const fn label(self) -> &'static str {
        LfoField::LABELS[self as usize]
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Aim {
    Gain,
    Send(usize),
    Level(usize),
    Wet(usize),
    Knob { row: usize, band: String, name: String },
}

impl Aim {
    pub fn knob(row: usize, band: &str, name: &str) -> Aim {
        Aim::Knob { row, band: String::from(band), name: String::from(name) }
    }

    pub fn row(&self) -> Option<usize> {
        match self {
            Aim::Level(row) | Aim::Wet(row) | Aim::Knob { row, .. } => Some(*row),
            Aim::Gain | Aim::Send(_) => None,
        }
    }

    pub fn mix_row(&self) -> Option<usize> {
        match self {
            Aim::Level(row) | Aim::Wet(row) => Some(*row),
            Aim::Gain | Aim::Send(_) | Aim::Knob { .. } => None,
        }
    }

    pub fn row_or_top(&self) -> usize {
        self.row().unwrap_or(0)
    }

    pub fn remap(&mut self, to: &impl Fn(usize) -> Option<usize>) -> bool {
        let (Aim::Level(row) | Aim::Wet(row) | Aim::Knob { row, .. }) = self else {
            return true;
        };
        to(*row).map(|landed| *row = landed).is_some()
    }

    pub fn words(&self) -> (&str, &str) {
        match self {
            Aim::Gain => (VOL, GAIN),
            Aim::Send(send) => (SEND, SEND_NAMES.get(*send).copied().unwrap_or("")),
            Aim::Level(_) => (MIX, "LEVEL"),
            Aim::Wet(_) => (MIX, "WET"),
            Aim::Knob { band, name, .. } => (band, name),
        }
    }

    fn seed(&self) -> u64 {
        let (band, name) = self.words();
        let declared = match self {
            Aim::Knob { .. } => MidiRow::lane_seed(band, name).or_else(|| coresynth::Params::lane_seed(band, name)).or_else(|| fx::Kind::ALL.into_iter().find_map(|kind| fx::Params::default_for(kind).lane_seed(band, name))),
            Aim::Gain | Aim::Send(_) | Aim::Level(_) | Aim::Wet(_) => None,
        };
        declared.unwrap_or_else(|| param::seed(band, name))
    }
}

pub fn moved_lfos(mut lfos: Vec<LaneLfo>, to: impl Fn(usize) -> Option<usize>) -> Vec<LaneLfo> {
    lfos.retain_mut(|lfo| lfo.aim.remap(&to));
    lfos
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Node {
    pub tick: i32,
    pub travel: f32,
    pub bend: f32,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Curve {
    pub aim: Aim,
    pub lfo: Option<LfoField>,
    pub nodes: Vec<Node>,
}

impl Curve {
    pub fn at(&self, beat: f64) -> f64 {
        travel_at(&self.nodes, beat)
    }
}

fn curve_at(curves: &mut Vec<Curve>, aim: &Aim, lfo: Option<LfoField>) -> usize {
    match curves.iter().position(|curve| curve.aim == *aim && curve.lfo == lfo) {
        Some(at) => at,
        None => {
            curves.push(Curve { aim: aim.clone(), lfo, nodes: Vec::new() });
            curves.len() - 1
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct LaneLfo {
    pub aim: Aim,
    pub seed: u64,
    pub amount: f64,
    pub shape: LfoShape,
    pub sync: bool,
    pub division: Division,
    pub rate_hz: f64,
    pub phase: f64,
    pub centre: f64,
    pub retrigger: bool,
    pub legato: bool,
    pub once: bool,
    pub clock: LfoClock,
    pub audio: bool,
    pub fade_ms: f64,
    pub lag_ms: f64,
    pub points: Drawn,
}

#[derive(Clone, Copy, Debug)]
pub struct Sweep {
    shape: LfoShape,
    seed: u64,
    amount: f64,
    phase: f64,
    centre: f64,
    fade: f64,
    grown: f64,
    once: bool,
    drawn: Drawn,
}

const LAST_TURN: f64 = 1.0 - 1e-9;

impl Sweep {
    pub fn travel(&self, cycles: f64) -> f64 {
        (self.centre + self.swing(cycles)).clamp(0.0, 1.0)
    }

    pub fn swing(&self, cycles: f64) -> f64 {
        let turned = if self.once { cycles.min(LAST_TURN) } else { cycles };
        self.amount * self.grown * 0.5 * sway(self.shape, self.seed, turned + self.phase, &self.drawn)
    }

    pub fn faded(self, seconds: f64) -> Sweep {
        let grown = if self.fade > 0.0 { (seconds / self.fade).clamp(0.0, 1.0) } else { 1.0 };
        Sweep { grown, ..self }
    }

    pub fn centre(&self) -> f64 {
        self.centre
    }
}

fn sway(shape: LfoShape, seed: u64, turns: f64, drawn: &Drawn) -> f64 {
    let turn = turns - turns.floor();
    match shape {
        LfoShape::Sine => (TAU * turns).sin(),
        LfoShape::Triangle => triangle(turns + 0.75),
        LfoShape::Saw => 1.0 - 2.0 * turn,
        LfoShape::Ramp => 2.0 * turn - 1.0,
        LfoShape::Square => square(turns),
        LfoShape::Random => sample_hold(turns.floor() as i64, seed),
        LfoShape::Walk => walk(turns.floor() as i64, turn, seed),
        LfoShape::Drawn => 2.0 * drawn.value_at(turns) - 1.0,
    }
}

fn walk(cycle: i64, turn: f64, seed: u64) -> f64 {
    let (from, to) = (sample_hold(cycle, seed), sample_hold(cycle + 1, seed));
    from + (to - from) * (0.5 - 0.5 * (PI * turn).cos())
}

pub const PITCH_BANDS: [&str; 4] = ["OSC 1", "OSC 2", "OSC 3", "OSC 4"];

const LAG_TIME_CONSTANTS: f64 = 5.0;

impl LaneLfo {
    pub const MIN_HZ: f64 = 0.05;
    pub const MAX_HZ: f64 = 32.0;
    pub const AUDIO_MAX_HZ: f64 = 2000.0;
    pub const MAX_FADE_MS: f64 = 10_000.0;
    pub const MAX_LAG_MS: f64 = 5000.0;

    pub fn blank(aim: Aim) -> LaneLfo {
        LaneLfo { seed: aim.seed(), aim, amount: 0.0, shape: LfoShape::Sine, sync: true, division: Division::Quarter, rate_hz: 1.0, phase: 0.0, centre: 0.5, retrigger: false, legato: false, once: false, clock: LfoClock::Song, audio: false, fade_ms: 0.0, lag_ms: 0.0, points: Drawn::RAMP }
    }

    pub fn pitch_knob(&self) -> Option<(usize, f64)> {
        let Aim::Knob { band, name, .. } = &self.aim else {
            return None;
        };
        let osc = PITCH_BANDS.iter().position(|known| known == band)?;
        let semitones = match name.as_str() {
            "TUNE" => 1.0,
            "FINE" => 0.01,
            _ => return None,
        };
        Some((osc, semitones))
    }

    pub fn audio_rate(&self) -> Option<(usize, f64)> {
        self.pitch_knob().filter(|_| self.audio)
    }

    pub fn set_audio(&mut self, on: bool) {
        self.audio = on && self.pitch_knob().is_some();
        if !self.audio {
            self.rate_hz = self.rate_hz.min(LaneLfo::MAX_HZ);
        }
    }

    pub fn hz(&self) -> f64 {
        if self.audio_rate().is_some() { self.rate_hz } else { self.rate_hz.min(LaneLfo::MAX_HZ) }
    }

    pub fn cycles_per_beat(&self, bpm: f64) -> f64 {
        if self.sync { 1.0 / self.division.beats() } else { self.hz() * 60.0 / bpm.max(1.0) }
    }

    pub fn cycles_per_frame(&self, frames_per_beat: f64, sample_rate: f64) -> f64 {
        if self.sync { 1.0 / (self.division.beats() * frames_per_beat) } else { self.hz() / sample_rate }
    }

    pub fn travel(&self, curves: &[Curve], notes: &[SongNote], at: f64, bpm: f64, share: f64) -> f64 {
        let (cycles, seconds) = self.elapsed(notes, at, bpm);
        self.travel_cycles(curves, at, cycles, seconds, share)
    }

    pub fn swing(&self, curves: &[Curve], notes: &[SongNote], at: f64, bpm: f64, share: f64) -> f64 {
        let (cycles, seconds) = self.elapsed(notes, at, bpm);
        self.sweep(curves, at, share).faded(seconds).swing(cycles)
    }

    pub fn restarting(&self) -> bool {
        self.clock == LfoClock::Note || self.retrigger
    }

    pub fn phrased(&self) -> bool {
        self.clock == LfoClock::Song && self.retrigger && self.legato
    }

    fn elapsed(&self, notes: &[SongNote], at: f64, bpm: f64) -> (f64, f64) {
        let since = if self.restarting() { at - struck(notes, self.phrased()).take_while(|start| *start <= at).last().unwrap_or(0.0) } else { at };
        (since * self.cycles_per_beat(bpm), since * 60.0 / bpm.max(1.0))
    }

    pub fn cycles_elapsed(&self, beats: f64, seconds: f64) -> f64 {
        if self.sync { beats / self.division.beats() } else { seconds * self.hz() }
    }

    pub fn travel_cycles(&self, curves: &[Curve], at: f64, cycles: f64, seconds: f64, share: f64) -> f64 {
        self.sweep(curves, at, share).faded(seconds).travel(cycles)
    }

    pub fn sweep(&self, curves: &[Curve], at: f64, share: f64) -> Sweep {
        let (amount, phase, centre) = self.resolved(curves, at);
        Sweep { shape: self.shape, seed: self.seed, amount: amount * share, phase, centre, fade: self.fade_ms / 1000.0, grown: 1.0, once: self.once, drawn: self.points }
    }

    pub fn lag_alpha(&self, seconds: f64) -> f64 {
        if self.lag_ms > 0.0 { 1.0 - (-LAG_TIME_CONSTANTS * seconds * 1000.0 / self.lag_ms).exp() } else { 1.0 }
    }

    fn resolved(&self, curves: &[Curve], at: f64) -> (f64, f64, f64) {
        let of = |field: LfoField, knob: f64| curves.iter().find(|curve| curve.lfo == Some(field) && curve.aim == self.aim).map_or(knob, |curve| curve.at(at));
        let amount = of(LfoField::Amount, self.amount);
        let phase = of(LfoField::Phase, self.phase);
        let centre = of(LfoField::Center, self.centre);
        (amount, phase, centre)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Hold {
    pub start: f64,
    pub reach: f64,
}

pub fn struck(notes: &[SongNote], phrased: bool) -> impl Iterator<Item = f64> + '_ {
    let mut reach = f64::NEG_INFINITY;
    notes.iter().filter_map(move |note| {
        let struck = note.articulation.join == Join::Play && (!phrased || note.start >= reach);
        reach = reach.max(note.end);
        struck.then_some(note.start)
    })
}

pub fn held(notes: &[SongNote]) -> Hold {
    let mut held = Hold { start: 0.0, reach: 0.0 };
    for note in notes {
        if note.start >= held.reach && note.articulation.join == Join::Play {
            held.start = note.start;
        }
        held.reach = held.reach.max(note.end);
    }
    held
}

pub fn travel_at(nodes: &[Node], beat: f64) -> f64 {
    let after = nodes.partition_point(|node| beats(node.tick) <= beat);
    match (after.checked_sub(1).map(|index| nodes[index]), nodes.get(after).copied()) {
        (Some(from), Some(to)) => bow(from, to, ((beat - beats(from.tick)) / beats(to.tick - from.tick)).clamp(0.0, 1.0)),
        (Some(held), None) | (None, Some(held)) => f64::from(held.travel),
        (None, None) => 0.0,
    }
}

const STEEPEST: f64 = 32.0;
const LEAST: f64 = 1e-10;
const SLACK: f64 = 1e-6;

fn bow(from: Node, to: Node, t: f64) -> f64 {
    bowed(f64::from(from.travel), f64::from(to.travel), from.bend, t)
}

pub fn bowed(low: f64, high: f64, bend: f32, t: f64) -> f64 {
    if bend == 0.0 {
        return low + (high - low) * t;
    }
    let steepness = STEEPEST.powf(f64::from(bend.abs()));
    let early = (bend > 0.0) == (high > low);
    let reached = if early { 1.0 - (1.0 - t).powf(steepness) } else { t.powf(steepness) };
    low + (high - low) * reached
}

fn same_lane(curve: &Curve, other: &Curve) -> bool {
    curve.aim == other.aim && curve.lfo == other.lfo
}

fn redundant(nodes: &[Node], index: usize) -> bool {
    let node = nodes[index];
    let before = index.checked_sub(1).map(|at| nodes[at]);
    match (before, nodes.get(index + 1).copied()) {
        (None, None) => false,
        (None, Some(next)) => next.travel == node.travel,
        (Some(prev), None) => prev.travel == node.travel,
        (Some(prev), Some(next)) => {
            let t = f64::from(node.tick - prev.tick) / f64::from(next.tick - prev.tick);
            prev.bend == 0.0 && node.bend == 0.0 && (f64::from(node.travel) - bow(prev, next, t)).abs() <= SLACK
        }
    }
}

pub fn bend_through(low: f64, high: f64, t: f64, travel: f64) -> f32 {
    let rise = high - low;
    if rise == 0.0 || t <= 0.0 || t >= 1.0 {
        return 0.0;
    }
    let reached = ((travel - low) / rise).clamp(LEAST, 1.0 - LEAST);
    let early = reached > t;
    let steepness = if early { (1.0 - reached).ln() / (1.0 - t).ln() } else { reached.ln() / t.ln() };
    let bend = steepness.max(1.0).log(STEEPEST).min(1.0);
    (if early == (rise > 0.0) { bend } else { -bend }) as f32
}

fn bend_between(nodes: &[Node], from: Node, to: Node) -> f32 {
    let after = nodes.partition_point(|node| node.tick <= from.tick);
    let (Some(start), Some(end)) = (after.checked_sub(1).map(|at| nodes[at]), nodes.get(after).copied()) else {
        return from.bend;
    };
    let early = (start.bend > 0.0) == (end.travel > start.travel);
    let whole_shape = if early { to.tick == end.tick } else { from.tick == start.tick };
    if start.bend == 0.0 || whole_shape {
        return start.bend;
    }
    let span = f64::from(end.tick - start.tick);
    let (a, b) = (f64::from(from.tick - start.tick) / span, f64::from(to.tick - start.tick) / span);
    let steepness = STEEPEST.powf(f64::from(start.bend.abs()));
    let (low, high) = if early { (1.0 - b, 1.0 - a) } else { (a, b) };
    let parallel = ((high.powf(steepness) - low.powf(steepness)) / (steepness * (high - low))).powf(1.0 / (steepness - 1.0));
    let knee = if early { 1.0 - parallel } else { parallel };
    bend_through(f64::from(from.travel), f64::from(to.travel), (knee - a) / (b - a), bow(start, end, knee))
}

fn cut(nodes: &[Node], at: [i32; 2]) -> Vec<Node> {
    let fresh = at.into_iter().filter(|tick| *tick >= 0 && nodes.iter().all(|node| node.tick != *tick)).map(|tick| Node { tick, travel: travel_at(nodes, beats(tick)) as f32, bend: 0.0 });
    let mut cut: Vec<Node> = nodes.iter().copied().chain(fresh).collect();
    cut.sort_by_key(|node| node.tick);
    cut.dedup_by_key(|node| node.tick);
    for index in 1..cut.len() {
        cut[index - 1].bend = bend_between(nodes, cut[index - 1], cut[index]);
    }
    cut
}

fn pinned(nodes: &[Node], start: i32, end: i32) -> Vec<Node> {
    let mut pinned = cut(nodes, [start - 1, end]);
    if let Some(pin) = pinned.iter_mut().find(|node| node.tick == start - 1) {
        pin.bend = nodes.iter().find(|node| node.tick == start - 1).map_or(0.0, |node| node.bend);
    }
    pinned
}

const FIT_SLACK: f64 = 0.03;
const FIT_LEAST: f64 = 0.02;

fn fit(points: &[(i32, f32)]) -> Option<f32> {
    let (first, last) = (points[0], points[points.len() - 1]);
    let (from, to) = (Node { tick: first.0, travel: first.1, bend: 0.0 }, Node { tick: last.0, travel: last.1, bend: 0.0 });
    let inside = &points[1..points.len() - 1];
    let at = |tick: i32| f64::from(tick - first.0) / f64::from(last.0 - first.0);
    let off = |bend: f32, point: &(i32, f32)| (bow(Node { bend, ..from }, to, at(point.0)) - f64::from(point.1)).abs();
    let holds = |bend: f32| inside.iter().all(|point| off(bend, point) <= FIT_SLACK * f64::from(point.1).max(FIT_LEAST)).then_some(bend);
    holds(0.0).or_else(|| {
        let furthest = inside.iter().max_by(|a, b| off(0.0, a).total_cmp(&off(0.0, b)))?;
        holds(bend_through(f64::from(first.1), f64::from(last.1), at(furthest.0), f64::from(furthest.1)))
    })
}

pub fn fitted(points: &[(i32, f32)]) -> Vec<Node> {
    let level = |index: usize| points.get(index).map(|point| point.1);
    let kept: Vec<(i32, f32)> = points.iter().enumerate().filter(|(index, point)| *index == 0 || level(index - 1) != Some(point.1) || level(index + 1) != Some(point.1)).map(|(_, point)| *point).collect();
    let Some(last) = kept.len().checked_sub(1) else {
        return Vec::new();
    };
    let mut nodes = Vec::new();
    let mut from = 0;
    while from < last {
        let (mut good, mut bend, mut bad) = (from + 1, 0.0, last + 1);
        let mut step = 2;
        while good < last && bad > last {
            let reach = (from + step).min(last);
            match fit(&kept[from..=reach]) {
                Some(held) => (good, bend) = (reach, held),
                None => bad = reach,
            }
            step *= 2;
        }
        while bad - good > 1 {
            let middle = (good + bad) / 2;
            match fit(&kept[from..=middle]) {
                Some(held) => (good, bend) = (middle, held),
                None => bad = middle,
            }
        }
        nodes.push(Node { tick: kept[from].0, travel: kept[from].1, bend });
        from = good;
    }
    nodes.push(Node { tick: kept[last].0, travel: kept[last].1, bend: 0.0 });
    nodes
}

#[derive(Clone, PartialEq, Debug)]
pub struct Automation {
    pub curves: Vec<Curve>,
    pub lfos: Vec<LaneLfo>,
}

impl Automation {
    pub const NONE: Automation = Automation { curves: Vec::new(), lfos: Vec::new() };

    pub fn lfo(&self, aim: &Aim) -> Option<&LaneLfo> {
        self.lfos.iter().find(|lfo| lfo.aim == *aim).filter(|lfo| lfo.amount > 0.0)
    }

    pub fn lfo_mut(&mut self, aim: &Aim) -> &mut LaneLfo {
        match self.lfos.iter().position(|lfo| lfo.aim == *aim) {
            Some(at) => &mut self.lfos[at],
            None => {
                self.lfos.push(LaneLfo::blank(aim.clone()));
                self.lfos.last_mut().unwrap()
            }
        }
    }

    pub fn remove_lfo(&mut self, aim: &Aim) {
        self.lfos.retain(|lfo| lfo.aim != *aim);
    }

    pub fn settle_lfos(&mut self, kept: Option<&Aim>) {
        self.lfos.retain(|lfo| lfo.amount > 0.0 || kept == Some(&lfo.aim));
    }

    pub fn curve(&self, aim: &Aim) -> Option<&Curve> {
        self.curves.iter().find(|curve| curve.lfo.is_none() && curve.aim == *aim)
    }

    pub fn move_rows(&mut self, to: impl Fn(usize) -> Option<usize>) {
        self.curves.retain_mut(|curve| curve.aim.remap(&to));
        self.lfos.retain_mut(|lfo| lfo.aim.remap(&to));
    }

    pub fn set_node(&mut self, aim: &Aim, lfo: Option<LfoField>, node: Node) {
        if lfo.is_none() {
            self.remove_lfo(aim);
        }
        self.lay_node(aim, lfo, node);
    }

    fn lay_node(&mut self, aim: &Aim, lfo: Option<LfoField>, node: Node) {
        let at = curve_at(&mut self.curves, aim, lfo);
        let nodes = &mut self.curves[at].nodes;
        match nodes.iter_mut().find(|held| held.tick == node.tick) {
            Some(held) => *held = node,
            None => {
                nodes.push(node);
                nodes.sort_by_key(|node| node.tick);
            }
        }
    }

    pub fn print(&mut self, aim: &Aim, nodes: Vec<Node>) {
        let until = nodes.last().map_or(-1, |node| node.tick);
        let after: Vec<Node> = self.curve(aim).map_or_else(Vec::new, |curve| curve.nodes.iter().filter(|node| node.tick > until).copied().collect());
        self.curves.retain(|curve| curve.aim != *aim);
        self.remove_lfo(aim);
        self.curves.push(Curve { aim: aim.clone(), lfo: None, nodes: nodes.into_iter().chain(after).collect() });
    }

    pub fn bend_node(&mut self, aim: &Aim, lfo: Option<LfoField>, tick: i32, bend: f32) {
        for curve in self.curves.iter_mut().filter(|curve| curve.aim == *aim && curve.lfo == lfo) {
            if let Some(node) = curve.nodes.iter_mut().find(|node| node.tick == tick) {
                node.bend = bend;
            }
        }
    }

    pub fn remove_node(&mut self, aim: &Aim, lfo: Option<LfoField>, tick: i32) {
        for curve in self.curves.iter_mut().filter(|curve| curve.aim == *aim && curve.lfo == lfo) {
            curve.nodes.retain(|node| node.tick != tick);
        }
        self.curves.retain(|curve| !curve.nodes.is_empty());
    }

    pub fn tidy_node(&mut self, aim: &Aim, tick: i32, base: f32) {
        let Some(curve) = self.curves.iter_mut().find(|curve| curve.lfo.is_none() && curve.aim == *aim) else {
            return;
        };
        let Some(at) = curve.nodes.iter().position(|node| node.tick == tick) else {
            return;
        };
        let mut without = curve.nodes.clone();
        let node = without.remove(at);
        let heard = if without.is_empty() { base } else { travel_at(&without, beats(tick)) as f32 };
        if heard == node.travel {
            curve.nodes = without;
        }
        self.curves.retain(|curve| !curve.nodes.is_empty());
    }

    pub fn hold(&mut self, aim: &Aim, tick: i32, travel: f32, next: Option<i32>, base: f32) {
        let nodes = self.curve(aim).map(|curve| curve.nodes.clone()).unwrap_or_default();
        let heard = |at: i32| if nodes.is_empty() { base } else { travel_at(&nodes, beats(at)) as f32 };
        let before = (tick > 0).then(|| (tick - 1, heard(tick - 1)));
        let after = next.map(|next| (next, heard(next)));
        if let Some((next, _)) = after {
            self.clear_curves(&|curve| curve.lfo.is_none() && curve.aim == *aim, tick + 1, next);
        }
        let steps: Vec<(i32, f32)> = before.into_iter().chain([(tick, travel)]).chain(after.into_iter().flat_map(|(next, kept)| [(next - 1, travel), (next, kept)])).collect();
        for &(at, travel) in &steps {
            self.set_node(aim, None, Node { tick: at, travel, bend: 0.0 });
        }
        for &(at, _) in &steps {
            self.tidy_node(aim, at, base);
        }
    }

    pub fn between(&self, start: i32, end: i32) -> (Vec<Curve>, Vec<Curve>) {
        let last = end - 1;
        let (mut curves, mut edges) = (Vec::new(), Vec::new());
        for curve in self.curves.iter().filter(|curve| !curve.nodes.is_empty()) {
            let cut = cut(&curve.nodes, [start, last]);
            let moved = |node: &Node| Node { tick: node.tick - start, ..*node };
            let held: Vec<Node> = cut.iter().filter(|node| (start..end).contains(&node.tick) && curve.nodes.iter().any(|kept| kept.tick == node.tick)).map(moved).collect();
            if !held.is_empty() {
                curves.push(Curve { nodes: held, ..curve.clone() });
            }
            let ends = cut.iter().filter(|node| node.tick == start || node.tick == last).map(|node| Node { bend: if node.tick == last { 0.0 } else { node.bend }, ..moved(node) });
            edges.push(Curve { nodes: ends.collect(), ..curve.clone() });
        }
        (curves, edges)
    }

    fn pin_around(&mut self, chosen: &impl Fn(&Curve) -> bool, start: i32, end: i32) -> Vec<Curve> {
        let mut pins = Vec::new();
        for curve in self.curves.iter_mut().filter(|curve| chosen(curve) && curve.nodes.iter().any(|node| node.tick < start || end <= node.tick)) {
            let pinned = pinned(&curve.nodes, start, end);
            let fresh = pinned.iter().filter(|pin| curve.nodes.iter().all(|node| node.tick != pin.tick)).copied().collect();
            pins.push(Curve { aim: curve.aim.clone(), lfo: curve.lfo, nodes: fresh });
            curve.nodes = pinned;
        }
        pins
    }

    pub fn replace(&mut self, start: i32, end: i32, edges: &[Curve], curves: &[Curve]) {
        self.replace_where(|_| true, start, end, edges, curves);
    }

    pub fn replace_where(&mut self, chosen: impl Fn(&Curve) -> bool, start: i32, end: i32, edges: &[Curve], curves: &[Curve]) {
        let pins = self.pin_around(&chosen, start, end);
        self.clear_curves(&chosen, start, end);
        self.lay_curves(edges, start);
        self.lay_curves(curves, start);
        for (fresh, at) in pins.iter().map(|pin| (pin, 0)).chain(edges.iter().map(|edge| (edge, start))) {
            let Some(index) = self.curves.iter().position(|curve| same_lane(curve, fresh)) else {
                continue;
            };
            let taken = |tick: i32| curves.iter().any(|curve| same_lane(curve, fresh) && curve.nodes.iter().any(|node| node.tick + start == tick));
            for tick in fresh.nodes.iter().map(|node| node.tick + at).filter(|&tick| !taken(tick)) {
                let nodes = &mut self.curves[index].nodes;
                if let Some(held) = nodes.iter().position(|node| node.tick == tick)
                    && redundant(nodes, held)
                {
                    nodes.remove(held);
                }
            }
        }
    }

    pub fn open(&mut self, at: i32, by: i32) {
        self.pin_edges(at, at);
        self.shift(at, by);
    }

    pub fn close(&mut self, start: i32, end: i32) {
        self.pin_edges(start, end);
        self.clear_curves(&|_| true, start, end);
        self.shift(end, start - end);
    }

    fn pin_edges(&mut self, start: i32, end: i32) {
        for curve in self.curves.iter_mut().filter(|curve| curve.nodes.iter().any(|node| node.tick >= start)) {
            curve.nodes = pinned(&curve.nodes, start, end);
        }
    }

    fn shift(&mut self, at: i32, by: i32) {
        for node in self.curves.iter_mut().flat_map(|curve| curve.nodes.iter_mut()).filter(|node| node.tick >= at) {
            node.tick += by;
        }
    }

    fn clear_curves(&mut self, chosen: &impl Fn(&Curve) -> bool, start: i32, end: i32) {
        for curve in self.curves.iter_mut().filter(|curve| chosen(curve)) {
            curve.nodes.retain(|node| node.tick < start || end <= node.tick);
        }
        self.curves.retain(|curve| !curve.nodes.is_empty());
    }

    pub fn lay_curves(&mut self, curves: &[Curve], at: i32) {
        for curve in curves {
            for node in &curve.nodes {
                self.lay_node(&curve.aim, curve.lfo, Node { tick: node.tick + at, ..*node });
            }
        }
    }
}

impl Track {
    pub fn reach(&self, end: i32) -> i32 {
        end + i32::from(self.clips.iter().all(|clip| clip.start != end))
    }
}

use sdk::dsp::envelope;
use crate::engine::mix_compensation;
use crate::face::CORE;
use crate::osc::{Cycle, naive_pulse, naive_wave};
use crate::params::{Envelope, Osc, Params, Shape, Trigger, WAVES, Wave};
use crate::sway::sway;
use sdk::draw::{DrawList, Rect, Tone};
use sdk::face::{self, Cell, Drawn, LABEL_SCALE, Sketch, draw_nodes};
use sdk::font;

const PAD: i32 = 10;
const GAP: i32 = 6;
const LINE: i32 = font::GLYPH_H + GAP;
const NODE: i32 = 5;
const ARM: i32 = 3;
const LEAST_W: f64 = 12.0;
const SUSTAIN_SHARE: f64 = 0.125;
const GRIP: f64 = 0.15;
const SNAP: f64 = 0.04;
const GUIDES: [f64; 3] = [0.25, 0.5, 0.75];
const LFO_BANDS: [&str; 2] = ["LFO 1", "LFO 2"];
const LFO_VALUES: [&str; 5] = ["SHAPE", "RATE", "MODE", "AMOUNT", "TARGET"];
const OSC_BANDS: [&str; Params::OSCS] = ["OSC 1", "OSC 2", "OSC 3", "OSC 4"];
const WAVE_NAMES: [&str; WAVES] = ["WAVE 1", "WAVE 2", "WAVE 3", "WAVE 4"];
const SHOWN_CYCLES: f64 = 2.0;
const LAST_TURN: f64 = 1.0 - 1e-9;
const TV_SEED: u64 = 0x5EED;
const DRAWN_HINT: &str = "Press the line to add a point, press a lit point to remove it; SHIFT-drag a segment to bend it, SHIFT-press it twice to straighten";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Which {
    Amp,
    Filter,
    Env1,
    Env2,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shown {
    Envelope(Which, Stage),
    Lfo(usize),
    Wave(usize, usize),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Delay,
    Attack,
    Decay,
    Sustain,
    Release,
}

impl Which {
    const ALL: [Which; 4] = [Which::Amp, Which::Filter, Which::Env1, Which::Env2];

    pub fn band(self) -> &'static str {
        match self {
            Which::Amp => "AMP ENV",
            Which::Filter => "FILTER ENV",
            Which::Env1 => "ENV 1",
            Which::Env2 => "ENV 2",
        }
    }

    fn envelope(self, p: &Params) -> &Envelope {
        match self {
            Which::Amp => &p.amp,
            Which::Filter => &p.filter.env,
            Which::Env1 => &p.motion.env1.env,
            Which::Env2 => &p.motion.env2.env,
        }
    }

    fn envelope_mut(self, p: &mut Params) -> &mut Envelope {
        match self {
            Which::Amp => &mut p.amp,
            Which::Filter => &mut p.filter.env,
            Which::Env1 => &mut p.motion.env1.env,
            Which::Env2 => &mut p.motion.env2.env,
        }
    }
}

impl Stage {
    const ALL: [Stage; 5] = [Stage::Delay, Stage::Attack, Stage::Decay, Stage::Sustain, Stage::Release];

    pub fn name(self) -> &'static str {
        match self {
            Stage::Delay => "DELAY",
            Stage::Attack => "ATTACK",
            Stage::Decay => "DECAY",
            Stage::Sustain => "SUSTAIN",
            Stage::Release => "RELEASE",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Stage::Delay => "The wait before the attack; set it in the DLY cell",
            Stage::Attack => "Drag to bend the attack, SHIFT-press to reset",
            Stage::Decay => "Drag to bend the decay, SHIFT-press to reset",
            Stage::Sustain => "Drag to set the sustain level, SHIFT-press to reset",
            Stage::Release => "Drag to bend the release, SHIFT-press to reset",
        }
    }

    fn index(self) -> usize {
        Stage::ALL.iter().position(|stage| *stage == self).unwrap_or(0)
    }

    fn flat(self) -> bool {
        self == Stage::Delay
    }

    fn bend(self, env: &Envelope) -> f64 {
        match self {
            Stage::Attack => env.attack_bend,
            Stage::Decay => env.decay_bend,
            Stage::Release => env.release_bend,
            Stage::Delay | Stage::Sustain => 0.0,
        }
    }

    fn set_bend(self, env: &mut Envelope, bend: f64) {
        match self {
            Stage::Attack => env.attack_bend = bend,
            Stage::Decay => env.decay_bend = bend,
            Stage::Release => env.release_bend = bend,
            Stage::Delay | Stage::Sustain => {}
        }
    }

    fn ends(self, env: &Envelope) -> (f64, f64) {
        match self {
            Stage::Delay => (0.0, 0.0),
            Stage::Attack => (0.0, 1.0),
            Stage::Decay => (1.0, env.sustain),
            Stage::Sustain => (env.sustain, env.sustain),
            Stage::Release => (env.sustain, 0.0),
        }
    }
}

pub fn stage_of(index: usize) -> Option<(Which, Stage)> {
    let cell = CORE.get(index)?;
    let which = Which::ALL.into_iter().find(|which| which.band() == cell.param.band)?;
    let stage = Stage::ALL.into_iter().find(|stage| stage.name() == cell.param.name)?;
    Some((which, stage))
}

pub fn shown_of(index: usize) -> Option<Shown> {
    if let Some((which, stage)) = stage_of(index) {
        return Some(Shown::Envelope(which, stage));
    }
    let cell = CORE.get(index)?;
    if let Some(lfo) = LFO_BANDS.iter().position(|band| *band == cell.param.band) {
        return Some(Shown::Lfo(lfo));
    }
    let osc = OSC_BANDS.iter().position(|band| *band == cell.param.band)?;
    let slot = WAVE_NAMES.iter().position(|name| *name == cell.param.name)?;
    Some(Shown::Wave(osc, slot))
}

fn named(band: &str, name: &str) -> Option<&'static Cell<Params>> {
    CORE.iter().find(|cell| cell.param.band == band && cell.param.name == name)
}

pub fn stage_cell(which: Which, stage: Stage) -> Option<usize> {
    CORE.iter().position(|cell| cell.param.band == which.band() && cell.param.name == stage.name())
}

pub fn straighten(p: &mut Params, which: Which, stage: Stage) {
    let env = which.envelope_mut(p);
    stage.set_bend(env, stage.bend(&Envelope::DEFAULT));
}

fn plot(inner: Rect) -> Rect {
    let values = inner.y + inner.h - PAD - font::GLYPH_H - face::HINT_LINES as i32 * LINE;
    Rect::new(inner.x + PAD, inner.y + PAD, inner.w - 2 * PAD, values - GAP - inner.y - PAD)
}

fn draw_hint(list: &mut DrawList, plot: Rect, values: i32, hint: &str) {
    for (index, line) in font::fitted(hint, font::glyphs_in(plot.w, LABEL_SCALE), face::HINT_LINES).iter().enumerate() {
        list.text(plot.x, values + (index as i32 + 1) * LINE, line, Tone::Ink75, LABEL_SCALE);
    }
}

fn y_of(plot: Rect, level: f64) -> i32 {
    plot.y + ((1.0 - level.clamp(0.0, 1.0)) * f64::from(plot.h - 1)).round() as i32
}

fn level_of(plot: Rect, y: i32) -> f64 {
    (1.0 - f64::from(y - plot.y) / f64::from((plot.h - 1).max(1))).clamp(0.0, 1.0)
}

fn snapped(bend: f64) -> f64 {
    if bend.abs() < SNAP { 0.0 } else { bend.clamp(-1.0, 1.0) }
}

struct Graph {
    plot: Rect,
    edges: [f64; 6],
}

impl Graph {
    fn new(plot: Rect, env: &Envelope) -> Graph {
        let width = f64::from(plot.w - 1);
        let sustained = width * SUSTAIN_SHARE;
        let timed = width - sustained;
        let least = LEAST_W.min(timed / 3.0);
        let free = timed - 3.0 * least;
        let times = [env.delay_ms, env.attack_ms, env.decay_ms, env.release_ms];
        let total: f64 = times.iter().sum();
        let share = |ms: f64, floor: f64| floor + free * ms / total;
        let delay = share(times[0], 0.0);
        let attack = delay + share(times[1], least);
        let decay = attack + share(times[2], least);
        let sustain = decay + sustained;
        Graph { plot, edges: [0.0, delay, attack, decay, sustain, width] }
    }

    fn span(&self, stage: Stage) -> (f64, f64) {
        let index = stage.index();
        (self.edges[index], self.edges[index + 1])
    }

    fn shown(&self, stage: Stage) -> bool {
        let (lo, hi) = self.span(stage);
        !stage.flat() || hi > lo
    }

    fn node(&self, index: usize) -> usize {
        if index == 1 && !self.shown(Stage::Delay) { 0 } else { index }
    }

    fn stage_at(&self, px: f64) -> Stage {
        Stage::ALL.into_iter().rev().find(|stage| px >= self.span(*stage).0).unwrap_or(Stage::Attack)
    }

    fn progress(&self, stage: Stage, px: f64) -> f64 {
        let (lo, hi) = self.span(stage);
        ((px - lo) / (hi - lo).max(1.0)).clamp(0.0, 1.0)
    }

    fn level_at(&self, env: &Envelope, px: f64) -> f64 {
        let stage = self.stage_at(px);
        let (from, to) = stage.ends(env);
        envelope::level(from, to, stage.bend(env), self.progress(stage, px))
    }

    fn corner(&self, env: &Envelope, index: usize) -> Rect {
        let level = match index {
            0 | 1 | 5 => 0.0,
            2 => 1.0,
            _ => env.sustain,
        };
        let x = self.plot.x + self.edges[index].round() as i32 - NODE / 2;
        let y = y_of(self.plot, level) - NODE / 2;
        Rect::new(x.clamp(self.plot.x, self.plot.x + self.plot.w - NODE), y.clamp(self.plot.y, self.plot.y + self.plot.h - NODE), NODE, NODE)
    }
}

#[derive(Clone, Copy)]
struct Drag {
    which: Which,
    stage: Stage,
    origin: (i32, i32),
    u: f64,
    armed: bool,
}

pub struct Tv {
    hover: Option<Stage>,
    drag: Option<Drag>,
    sketch: Sketch,
    sketching: Option<Shown>,
}

impl Default for Tv {
    fn default() -> Tv {
        Tv::new()
    }
}

impl Tv {
    pub fn new() -> Tv {
        Tv { hover: None, drag: None, sketch: Sketch::new(), sketching: None }
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some() || self.sketch.dragging()
    }

    pub fn sketched(&self) -> Option<Shown> {
        self.sketching.filter(|_| self.sketch.dragging())
    }

    pub fn down_sketch(&mut self, x: i32, y: i32, inner: Rect, p: &mut Params, shown: Shown, shift: bool, clicks: i32) -> bool {
        let plot = plot(inner);
        let Some(drawn) = sketchable(p, shown).filter(|_| plot.contains(x, y)) else {
            return false;
        };
        if self.sketching != Some(shown) {
            self.sketch.node = None;
        }
        self.sketching = Some(shown);
        self.sketch.down(x, y, plot, drawn, shift, clicks)
    }

    fn node(&self, shown: Shown) -> Option<usize> {
        self.sketch.node.filter(|_| self.sketching == Some(shown))
    }

    pub fn dragged(&self) -> Option<(Which, Stage)> {
        self.drag.map(|drag| (drag.which, drag.stage))
    }

    fn lit(&self) -> Option<Stage> {
        self.drag.map(|drag| drag.stage).or(self.hover)
    }

    pub fn stage_at(x: i32, y: i32, inner: Rect, p: &Params, which: Which) -> Option<Stage> {
        let plot = plot(inner);
        plot.contains(x, y).then(|| Graph::new(plot, which.envelope(p)).stage_at(f64::from(x - plot.x)))
    }

    pub fn hover(&mut self, point: Option<(i32, i32)>, inner: Rect, shown: Option<(Which, &Params)>) -> bool {
        let hit = match (point, shown) {
            (Some((x, y)), Some((which, p))) => Tv::stage_at(x, y, inner, p, which),
            _ => None,
        };
        let changed = hit != self.hover;
        self.hover = hit;
        changed
    }

    pub fn down(&mut self, x: i32, y: i32, inner: Rect, p: &Params, which: Which) -> bool {
        let Some(stage) = Tv::stage_at(x, y, inner, p, which) else {
            return false;
        };
        let plot = plot(inner);
        let u = Graph::new(plot, which.envelope(p)).progress(stage, f64::from(x - plot.x)).clamp(GRIP, 1.0 - GRIP);
        self.hover = Some(stage);
        self.drag = Some(Drag { which, stage, origin: (x, y), u, armed: false });
        true
    }

    pub fn moved(&mut self, x: i32, y: i32, inner: Rect, p: &mut Params) -> bool {
        if let Some(shown) = self.sketched() {
            return sketchable(p, shown).is_some_and(|drawn| self.sketch.moved(x, y, plot(inner), drawn));
        }
        let Some(drag) = &mut self.drag else {
            return false;
        };
        drag.armed |= (x - drag.origin.0).abs() >= ARM || (y - drag.origin.1).abs() >= ARM;
        if !drag.armed {
            return false;
        }
        let level = level_of(plot(inner), y);
        let env = drag.which.envelope_mut(p);
        if drag.stage == Stage::Sustain {
            env.sustain = level;
            return true;
        }
        let (from, to) = drag.stage.ends(env);
        match envelope::bend_through(from, to, drag.u, level) {
            Some(bend) => {
                drag.stage.set_bend(env, snapped(bend));
                true
            }
            None => false,
        }
    }

    pub fn up(&mut self, p: Option<&mut Params>) -> bool {
        if let Some(shown) = self.sketched() {
            let mut scratch = Drawn::RAMP;
            return self.sketch.up(p.and_then(|p| sketchable(p, shown)).unwrap_or(&mut scratch));
        }
        self.drag.take().is_some()
    }

    pub fn draw(&self, list: &mut DrawList, inner: Rect, p: &Params, shown: Shown, preview: Option<Wave>, desc: &str) {
        match shown {
            Shown::Envelope(which, stage) => self.draw_envelope(list, inner, p, which, stage, desc),
            Shown::Lfo(index) => draw_lfo(list, inner, p, index, desc, self.node(shown)),
            Shown::Wave(osc, slot) => draw_wave(list, inner, p, osc, slot, preview, desc, self.node(shown)),
        }
    }

    fn draw_envelope(&self, list: &mut DrawList, inner: Rect, p: &Params, which: Which, stage: Stage, desc: &str) {
        let env = which.envelope(p);
        let plot = plot(inner);
        let graph = Graph::new(plot, env);
        let lit = self.lit().unwrap_or(stage);
        for level in GUIDES {
            list.fill(Rect::new(plot.x, y_of(plot, level), plot.w, 1), Tone::Ink25);
        }
        let floor = plot.y + plot.h;
        for column in 0..plot.w {
            let px = f64::from(column);
            let (a, b) = (y_of(plot, graph.level_at(env, px)), y_of(plot, graph.level_at(env, px + 1.0)));
            let (top, bottom) = (a.min(b), a.max(b));
            let wash = if graph.stage_at(px) == lit { Tone::Ink50 } else { Tone::Ink25 };
            list.fill(Rect::new(plot.x + column, top, 1, floor - top), wash);
            list.fill(Rect::new(plot.x + column, top, 1, bottom - top + 1), Tone::Ink100);
        }
        let (from, to) = (graph.node(lit.index()), graph.node(lit.index() + 1));
        for index in (0..6).filter(|index| graph.node(*index) == *index) {
            let rect = graph.corner(env, index);
            let on = index == from || index == to;
            list.fill(rect, if on { Tone::Ink100 } else { Tone::Ground });
            list.frame(rect, 1, Tone::Ink100);
        }
        let values = floor + GAP;
        let shown: Vec<Stage> = Stage::ALL.into_iter().filter(|stage| graph.shown(*stage)).collect();
        let column = plot.w / (shown.len() as i32 + 1);
        list.text(plot.x, values, which.band(), Tone::Ink50, LABEL_SCALE);
        for (index, stage) in shown.into_iter().enumerate() {
            let Some(cell) = named(which.band(), stage.name()) else {
                continue;
            };
            let tone = if stage == lit { Tone::Ink100 } else { Tone::Ink50 };
            let value = face::spelled(cell, p);
            let labelled = format!("{} {value}", cell.label);
            let text = if font::width(&labelled, LABEL_SCALE) + GAP > column { value } else { labelled };
            list.text(plot.x + (index as i32 + 1) * column, values, &text, tone, LABEL_SCALE);
        }
        let hint = match self.lit() {
            Some(stage) => stage.hint(),
            None => desc,
        };
        draw_hint(list, plot, values, hint);
    }
}

fn sketchable(p: &mut Params, shown: Shown) -> Option<&mut Drawn> {
    match shown {
        Shown::Lfo(index) => {
            let lfo = p.motion.lfo_mut(index);
            (lfo.shape == Shape::Drawn).then_some(&mut lfo.points)
        }
        Shown::Wave(osc, slot) => {
            let osc = p.osc_mut(osc);
            (osc.shapes[slot] == Wave::Drawn).then_some(&mut osc.points)
        }
        Shown::Envelope(..) => None,
    }
}

fn draw_lfo(list: &mut DrawList, inner: Rect, p: &Params, index: usize, desc: &str, selected: Option<usize>) {
    let lfo = p.motion.lfos()[index];
    let plot = plot(inner);
    for level in GUIDES {
        list.fill(Rect::new(plot.x, y_of(plot, level), plot.w, 1), Tone::Ink25);
    }
    let once = lfo.trigger == Trigger::Once;
    let drawn = lfo.shape == Shape::Drawn;
    let cycles = if drawn { 1.0 } else { SHOWN_CYCLES };
    let wave_at = |column: i32| {
        let t = f64::from(column) / f64::from((plot.w - 1).max(1)) * cycles;
        let turns = if drawn { t } else { lfo.phase + if once { t.min(LAST_TURN) } else { t } };
        let wave = sway(lfo.shape, turns, TV_SEED, &lfo.points);
        (y_of(plot, 0.5 + 0.5 * wave), y_of(plot, 0.5 + 0.5 * wave * lfo.amount))
    };
    for column in 0..plot.w {
        let ((faint_a, swung_a), (faint_b, swung_b)) = (wave_at(column), wave_at(column + 1));
        list.fill(Rect::new(plot.x + column, faint_a.min(faint_b), 1, (faint_a - faint_b).abs() + 1), Tone::Ink25);
        list.fill(Rect::new(plot.x + column, swung_a.min(swung_b), 1, (swung_a - swung_b).abs() + 1), Tone::Ink100);
    }
    if drawn {
        draw_nodes(list, plot, &lfo.points, selected);
    }
    let values = plot.y + plot.h + GAP;
    let band = LFO_BANDS[index];
    let column = plot.w / (LFO_VALUES.len() as i32 + 1);
    let room = font::glyphs_in(column - GAP, LABEL_SCALE);
    list.text(plot.x, values, band, Tone::Ink50, LABEL_SCALE);
    for (slot, name) in LFO_VALUES.into_iter().enumerate() {
        let name = if name == "RATE" && lfo.sync { "DIVISION" } else { name };
        let Some(cell) = named(band, name) else {
            continue;
        };
        list.text(plot.x + (slot as i32 + 1) * column, values, &font::shortened(&face::spelled(cell, p), room), Tone::Ink100, LABEL_SCALE);
    }
    draw_hint(list, plot, values, if drawn { DRAWN_HINT } else { desc });
}

fn pictured(wave: Wave, phase: f64, pulse_width: f64, cycle: &Cycle) -> f64 {
    match wave {
        Wave::Square => naive_pulse(phase, pulse_width),
        _ => naive_wave(wave, phase, pulse_width, cycle),
    }
}

fn draw_cycle(list: &mut DrawList, plot: Rect, drawn: &Drawn, selected: Option<usize>) {
    let y_at = |column: i32| y_of(plot, drawn.value_at((f64::from(column) / f64::from((plot.w - 1).max(1))).min(LAST_TURN)));
    for column in 0..plot.w {
        let (a, b) = (y_at(column), y_at(column + 1));
        list.fill(Rect::new(plot.x + column, a.min(b), 1, (a - b).abs() + 1), Tone::Ink100);
    }
    draw_nodes(list, plot, drawn, selected);
}

fn draw_mix(list: &mut DrawList, plot: Rect, osc: &Osc, slot: usize) {
    let pulse_width = if osc.pure() { osc.pulse_width } else { 0.5 };
    let gain = mix_compensation(osc.levels) * if osc.invert { -1.0 } else { 1.0 };
    let cycle = Cycle::of(&osc.points);
    let phase_at = |column: i32| (f64::from(column) / f64::from((plot.w - 1).max(1)) * SHOWN_CYCLES).fract();
    let shape_at = |column: i32| pictured(osc.shapes[slot], phase_at(column), pulse_width, &cycle);
    let mix_at = |column: i32| gain * osc.shapes.iter().zip(osc.levels).map(|(wave, level)| level * pictured(*wave, phase_at(column), pulse_width, &cycle)).sum::<f64>();
    let peak = (0..=plot.w).map(|column| shape_at(column).abs().max(mix_at(column).abs())).fold(1.0, f64::max);
    let y_at = |value: f64| y_of(plot, 0.5 + 0.5 * value / peak);
    let middle = y_at(0.0);
    for column in 0..plot.w {
        let mix = y_at(mix_at(column));
        list.fill(Rect::new(plot.x + column, mix.min(middle), 1, (mix - middle).abs() + 1), Tone::Ink25);
        let (a, b) = (y_at(shape_at(column)), y_at(shape_at(column + 1)));
        list.fill(Rect::new(plot.x + column, a.min(b), 1, (a - b).abs() + 1), Tone::Ink100);
    }
}

fn draw_wave(list: &mut DrawList, inner: Rect, p: &Params, index: usize, slot: usize, preview: Option<Wave>, desc: &str, selected: Option<usize>) {
    let mut osc = p.oscs()[index];
    if let Some(wave) = preview {
        osc.shapes[slot] = wave;
    }
    let plot = plot(inner);
    for level in GUIDES {
        list.fill(Rect::new(plot.x, y_of(plot, level), plot.w, 1), Tone::Ink25);
    }
    let drawn = osc.shapes[slot] == Wave::Drawn;
    if drawn {
        draw_cycle(list, plot, &osc.points, selected);
    } else {
        draw_mix(list, plot, &osc, slot);
    }
    let values = plot.y + plot.h + GAP;
    let band = OSC_BANDS[index];
    let column = plot.w / (WAVES as i32 + 1);
    let room = font::glyphs_in(column - GAP, LABEL_SCALE);
    list.text(plot.x, values, band, Tone::Ink50, LABEL_SCALE);
    for (k, name) in WAVE_NAMES.into_iter().enumerate() {
        let Some(cell) = named(band, name) else {
            continue;
        };
        let tone = if k == slot { Tone::Ink100 } else { Tone::Ink50 };
        list.text(plot.x + (k as i32 + 1) * column, values, &font::shortened(&format!("{} {}", osc.shapes[k].short(), face::spelled(cell, p)), room), tone, LABEL_SCALE);
    }
    draw_hint(list, plot, values, if drawn { DRAWN_HINT } else { desc });
}

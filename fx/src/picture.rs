mod duck;
mod eq;
mod filter;
mod freak;
mod lfo;
mod motion;
mod ramp;
mod space;
mod suppress;

use sdk::draw::{DrawList, Rect, Tone};
use sdk::face::{Cell, LABEL_SCALE, READOUT_SCALE, Sketch, VALUE_SCALE};
use sdk::font;
use sdk::spectrum::{CURVE, CURVE_LOW_HZ, CURVE_PER_OCTAVE};
use crate::Params;
use crate::params::{FREAK_BANDS, Slot};

pub(crate) use duck::{draw_duck, draw_duck_tv};
pub(crate) use eq::draw_curve;
pub(crate) use filter::draw_filter;
pub(crate) use freak::draw_banded;
pub(crate) use lfo::draw_shape;
pub(crate) use ramp::draw_ramp;
pub(crate) use space::draw_space;
pub(crate) use suppress::draw_suppress;

const GRAB: i32 = 12;
const FINE: i32 = 10;
const ARM: i32 = 4;
const PLOT_INSET: i32 = 10;
const DASH: i32 = 3;
const DASH_PERIOD: i32 = 6;
const GR_FULL_DB: f32 = 24.0;
const LABEL_PAD: i32 = 4;
const GAP: i32 = 6;

fn cut_at(cuts: &[f32; CURVE], hz: f64) -> f32 {
    let position = ((hz / CURVE_LOW_HZ).log2() * CURVE_PER_OCTAVE).clamp(0.0, (CURVE - 1) as f64);
    let (below, frac) = (position.floor() as usize, position.fract() as f32);
    let above = (below + 1).min(CURVE - 1);
    cuts[below] + (cuts[above] - cuts[below]) * frac
}

fn plot(inner: Rect) -> Rect {
    Rect::new(inner.x + PLOT_INSET, inner.y + PLOT_INSET, inner.w - 2 * PLOT_INSET, inner.h - 2 * PLOT_INSET)
}

fn caption_foot(inner: Rect) -> i32 {
    inner.y + inner.h - PLOT_INSET - font::GLYPH_H * LABEL_SCALE
}

fn caption_line(inner: Rect) -> i32 {
    caption_foot(inner) - GAP - font::GLYPH_H * READOUT_SCALE
}

fn plot_over_caption(inner: Rect) -> Rect {
    Rect::new(inner.x + PLOT_INSET, inner.y + PLOT_INSET, inner.w - 2 * PLOT_INSET, caption_line(inner) - GAP - inner.y - PLOT_INSET)
}

fn captioned<P>(list: &mut DrawList, inner: Rect, cell: &Cell<P>, value: &str) -> Rect {
    let (foot, line, plot) = (caption_foot(inner), caption_line(inner), plot_over_caption(inner));
    let base = line + font::GLYPH_H * (READOUT_SCALE - LABEL_SCALE);
    let mut x = plot.x;
    for (text, tone) in [(cell.param.band, Tone::Ink50), (cell.param.name, Tone::Ink75)] {
        list.text(x, base, text, tone, LABEL_SCALE);
        x += font::width(text, LABEL_SCALE) + GAP;
    }
    x += GAP;
    list.text(x, line, &font::shortened(value, font::glyphs_in(plot.x + plot.w - x, READOUT_SCALE)), Tone::Ink100, READOUT_SCALE);
    list.text(plot.x, foot, &font::shortened(cell.desc, font::glyphs_in(plot.w, LABEL_SCALE)), Tone::Ink75, LABEL_SCALE);
    plot
}

pub(crate) fn draw_reduction(list: &mut DrawList, inner: Rect, reduction_db: f32) {
    for db in [6.0, 12.0, 18.0] {
        let x = inner.x + (f64::from(inner.w) * db / f64::from(GR_FULL_DB)).round() as i32;
        list.fill(Rect::new(x, inner.y, 1, inner.h), Tone::Ink25);
    }
    let width = (f64::from(inner.w) * f64::from((reduction_db / GR_FULL_DB).clamp(0.0, 1.0))).round() as i32;
    if width > 0 {
        list.fill(Rect::new(inner.x, inner.y, width, inner.h), Tone::Ink75);
        list.fill(Rect::new(inner.x + width - 1, inner.y, 1, inner.h), Tone::Ink100);
    }
    let text = if reduction_db < 0.05 { String::from("0.0 dB") } else { format!("-{reduction_db:.1} dB") };
    let x = inner.x + (inner.w - font::width(&text, VALUE_SCALE)) / 2;
    let y = inner.y + (inner.h - font::GLYPH_H * VALUE_SCALE) / 2;
    list.text_split(x, y, &text, inner.x + width, Tone::Ground, Tone::Ink100, VALUE_SCALE);
}

#[derive(Clone, Copy)]
enum Drag {
    Handle { band: usize, q0: f64 },
    Crossover { split: usize, before: [f64; FREAK_BANDS - 1] },
    Gain { band: usize, anchors: [f64; 2] },
    Cut { unit: usize },
    Idle,
}

pub struct Picture {
    pub band: Option<usize>,
    pub(super) sketch: Sketch,
    drag: Option<Drag>,
    origin: (i32, i32),
    armed: bool,
    wheel: (Option<usize>, i32),
}

impl Default for Picture {
    fn default() -> Picture {
        Picture::new()
    }
}

impl Picture {
    pub fn new() -> Picture {
        Picture { band: None, sketch: Sketch::new(), drag: None, origin: (0, 0), armed: false, wheel: (None, 0) }
    }

    pub fn reset(&mut self) {
        *self = Picture::new();
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some() || self.sketch.dragging()
    }

    pub fn down(&mut self, x: i32, y: i32, inner: Rect, slot: &mut Slot, shift: bool, clicks: i32) -> bool {
        self.origin = (x, y);
        self.armed = false;
        match &mut slot.params {
            Params::Eq(eq) => self.down_eq(x, y, inner, eq),
            Params::VolumeLfo(lfo) => self.sketch.down(x, y, plot(inner), &mut lfo.drawn, shift, clicks),
            Params::Freak(p) => self.down_banded(x, y, inner, p, clicks),
            Params::Motion(p) => self.down_banded(x, y, inner, p, clicks),
            Params::Suppressor(p) => self.down_banded(x, y, inner, p, clicks),
            _ => false,
        }
    }

    pub fn right(&mut self, x: i32, y: i32, inner: Rect, slot: &mut Slot) -> bool {
        match &mut slot.params {
            Params::Freak(p) => self.right_banded(x, y, inner, p),
            Params::Motion(p) => self.right_banded(x, y, inner, p),
            Params::Suppressor(p) => self.right_banded(x, y, inner, p),
            _ => false,
        }
    }

    pub fn moved(&mut self, x: i32, y: i32, inner: Rect, slot: &mut Slot, command: bool, shift: bool) -> bool {
        if self.sketch.dragging() {
            return match &mut slot.params {
                Params::VolumeLfo(lfo) => self.sketch.moved(x, y, plot(inner), &mut lfo.drawn),
                _ => false,
            };
        }
        let (dx, dy) = (x - self.origin.0, y - self.origin.1);
        self.armed |= dx.abs() >= ARM || dy.abs() >= ARM;
        let armed = self.armed;
        if let Some(Drag::Cut { unit }) = self.drag {
            return armed && slot.params.filtered_mut().is_some_and(|filtered| filter::drag(x, y, inner, filtered.filter_mut(unit)));
        }
        match (self.drag, &mut slot.params) {
            (Some(Drag::Handle { .. }), Params::Eq(eq)) => self.moved_handle(x, y, dy, inner, command, eq),
            (Some(Drag::Crossover { .. }), Params::Freak(p)) if armed => self.moved_crossover(x, inner, shift, p),
            (Some(Drag::Gain { .. }), Params::Freak(p)) if armed => self.moved_gain(dy, command, shift, p),
            (Some(Drag::Crossover { .. }), Params::Motion(p)) if armed => self.moved_crossover(x, inner, shift, p),
            (Some(Drag::Gain { .. }), Params::Motion(p)) if armed => self.moved_gain(dy, command, shift, p),
            (Some(Drag::Crossover { .. }), Params::Suppressor(p)) if armed => self.moved_crossover(x, inner, shift, p),
            (Some(Drag::Gain { .. }), Params::Suppressor(p)) if armed => self.moved_gain(dy, command, shift, p),
            _ => false,
        }
    }

    pub fn wheel(&mut self, dy: i32, slot: &mut Slot) -> bool {
        match &mut slot.params {
            Params::Eq(eq) => self.wheel_eq(dy, eq),
            _ => false,
        }
    }

    pub fn up(&mut self, slot: Option<&mut Slot>) -> bool {
        match (self.drag, slot) {
            (_, Some(Slot { params: Params::VolumeLfo(lfo), .. })) if self.sketch.dragging() => return self.sketch.up(&mut lfo.drawn),
            (Some(Drag::Crossover { split, before }), Some(Slot { params: Params::Freak(p), .. })) => self.release_crossover(split, before, p),
            (Some(Drag::Crossover { split, before }), Some(Slot { params: Params::Motion(p), .. })) => self.release_crossover(split, before, p),
            (Some(Drag::Crossover { split, before }), Some(Slot { params: Params::Suppressor(p), .. })) => self.release_crossover(split, before, p),
            _ => {}
        }
        self.drag.take().is_some()
    }
}

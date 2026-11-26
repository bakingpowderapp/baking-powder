use sdk::draw::{DrawList, Rect, Tone};
use sdk::face::Format::{Millis, TwoPlaces};
use sdk::face::{self, Cell, LABEL_SCALE, spell};
use sdk::font;
use crate::params::Time;
use super::{LABEL_PAD, captioned, plot};

const GUIDES: [f64; 3] = [0.25, 0.5, 0.75];
const LEAST_SHARE: f64 = 0.2;

fn y_of(plot: Rect, level: f64) -> i32 {
    plot.y + ((1.0 - level.clamp(0.0, 1.0)) * f64::from(plot.h - 1)).round() as i32
}

pub(crate) fn draw_ramp(list: &mut DrawList, inner: Rect, caption: Option<(&Cell<Time>, &str)>, p: &Time, speed: f32, phase: f32) {
    let area = caption.map_or(plot(inner), |(cell, value)| captioned(list, inner, cell, value));
    let text_h = font::GLYPH_H * LABEL_SCALE;
    let plot = Rect::new(area.x, area.y, area.w, area.h - text_h - LABEL_PAD);
    let floor = plot.y + plot.h;
    for level in GUIDES {
        list.fill(Rect::new(plot.x, y_of(plot, level), plot.w, 1), Tone::Ink25);
    }
    let total = p.stop_ms + p.start_ms;
    let share = if total > 0.0 { (p.stop_ms / total).clamp(LEAST_SHARE, 1.0 - LEAST_SHARE) } else { 0.5 };
    let split = plot.x + (share * f64::from(plot.w - 1)).round() as i32;
    let stop_w = f64::from((split - plot.x - 1).max(1));
    let start_w = f64::from((plot.x + plot.w - split - 1).max(1));
    let c = p.curve;
    for column in 0..plot.w {
        let px = plot.x + column;
        let s = if px < split { 1.0 - (f64::from(px - plot.x) / stop_w).powf(c) } else { 1.0 - (1.0 - f64::from(px - split) / start_w).powf(c) };
        let y = y_of(plot, s);
        list.fill(Rect::new(px, y, 1, floor - y), Tone::Ink25);
        list.fill(Rect::new(px, y, 1, 1), Tone::Ink100);
    }
    list.fill(Rect::new(split, plot.y, 1, plot.h), Tone::Ink50);
    if phase >= 0.0 {
        let s = f64::from(speed).clamp(0.0, 1.0);
        let x = if phase < 0.5 { plot.x + ((1.0 - s).powf(1.0 / c) * stop_w).round() as i32 } else { split + ((1.0 - (1.0 - s).powf(1.0 / c)) * start_w).round() as i32 };
        let y = y_of(plot, s);
        face::draw_dot(list, (x - face::DOT / 2).clamp(plot.x, plot.x + plot.w - face::DOT), (y - face::DOT / 2).clamp(plot.y, floor - face::DOT));
    }
    let foot = floor + LABEL_PAD;
    list.text(plot.x, foot, &format!("STOP {}", spell(Millis, p.stop_ms)), Tone::Ink75, LABEL_SCALE);
    list.text(split + LABEL_PAD, foot, &format!("START {}", spell(Millis, p.start_ms)), Tone::Ink75, LABEL_SCALE);
    let curve = format!("CURVE {}", spell(TwoPlaces, p.curve));
    list.text(plot.x + plot.w - LABEL_PAD - font::width(&curve, LABEL_SCALE), plot.y + LABEL_PAD, &curve, Tone::Ink75, LABEL_SCALE);
}

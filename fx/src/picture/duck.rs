use sdk::draw::{DrawList, Rect, Tone};
use sdk::face::{Cell, READOUT_SCALE};
use sdk::font;
use super::{GR_FULL_DB, captioned};

const SIGNAL_FLOOR_DB: f32 = -60.0;
const SIGNAL_GRID_DB: [f32; 4] = [-48.0, -36.0, -24.0, -12.0];

pub(crate) fn draw_duck_tv<P>(list: &mut DrawList, inner: Rect, signal_db: f32, reduction_db: f32, deepest_db: f32, threshold_db: f32, cell: &Cell<P>, value: &str) {
    let plot = captioned(list, inner, cell, value);
    list.frame(plot, 1, Tone::Ink25);
    draw_duck(list, Rect::new(plot.x + 1, plot.y + 1, plot.w - 2, plot.h - 2), signal_db, reduction_db, deepest_db, threshold_db, READOUT_SCALE);
}

pub(crate) fn draw_duck(list: &mut DrawList, inner: Rect, signal_db: f32, reduction_db: f32, deepest_db: f32, threshold_db: f32, scale: i32) {
    let half = inner.h / 2;
    let (top, bottom) = (Rect::new(inner.x, inner.y, inner.w, half), Rect::new(inner.x, inner.y + half, inner.w, inner.h - half));
    let across = |db: f32| (f64::from(inner.w) * f64::from(((db - SIGNAL_FLOOR_DB) / -SIGNAL_FLOOR_DB).clamp(0.0, 1.0))).round() as i32;
    for db in SIGNAL_GRID_DB {
        list.fill(Rect::new(top.x + across(db), top.y, 1, top.h), Tone::Ink25);
    }
    let heard = across(signal_db);
    if heard > 0 {
        list.fill(Rect::new(top.x, top.y, heard, top.h), Tone::Ink75);
        list.fill(Rect::new(top.x + heard - 1, top.y, 1, top.h), Tone::Ink100);
    }
    list.fill(Rect::new(top.x + across(threshold_db) - 1, top.y, 2, top.h), Tone::Ink100);
    let text = if signal_db <= SIGNAL_FLOOR_DB { String::from("-inf dB") } else { format!("{signal_db:+.1} dB") };
    let x = top.x + (top.w - font::width(&text, scale)) / 2;
    let y = top.y + (top.h - font::GLYPH_H * scale) / 2;
    list.text_split(x, y, &text, top.x + heard, Tone::Ground, Tone::Ink100, scale);
    list.fill(Rect::new(inner.x, bottom.y, inner.w, 1), Tone::Ink25);
    for db in [6.0, 12.0, 18.0] {
        let x = bottom.x + bottom.w - (f64::from(bottom.w) * db / f64::from(GR_FULL_DB)).round() as i32;
        list.fill(Rect::new(x, bottom.y, 1, bottom.h), Tone::Ink25);
    }
    let deepest_db = deepest_db.max(reduction_db);
    let down = |db: f32| (f64::from(bottom.w) * f64::from((db / GR_FULL_DB).clamp(0.0, 1.0))).round() as i32;
    let (pushed, reached) = (down(reduction_db), down(deepest_db));
    let edge = bottom.x + bottom.w - pushed;
    if reached > pushed {
        list.fill(Rect::new(edge - (reached - pushed), bottom.y, reached - pushed, bottom.h), Tone::Ink25);
    }
    if pushed > 0 {
        list.fill(Rect::new(edge, bottom.y, pushed, bottom.h), Tone::Ink75);
        list.fill(Rect::new(edge, bottom.y, 1, bottom.h), Tone::Ink100);
    }
    if reached > 0 {
        list.fill(Rect::new(bottom.x + bottom.w - reached, bottom.y, 2, bottom.h), Tone::Ink100);
    }
    let text = if deepest_db < 0.05 { String::from("0.0 dB") } else { format!("-{deepest_db:.1} dB") };
    let x = bottom.x + (bottom.w - font::width(&text, scale)) / 2;
    let y = bottom.y + (bottom.h - font::GLYPH_H * scale) / 2;
    list.text_split(x, y, &text, edge, Tone::Ink100, Tone::Ground, scale);
}

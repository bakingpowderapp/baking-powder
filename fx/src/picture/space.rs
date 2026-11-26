use sdk::draw::{DrawList, Rect, Tone};
use sdk::face::Format::Hertz;
use sdk::face::{Cell, LABEL_SCALE, spell};
use sdk::font;
use crate::params::Space;
use sdk::spectrum::{self, CURVE, Spectrum};
use super::eq::x_of_hz;
use super::{GR_FULL_DB, LABEL_PAD, captioned, cut_at, plot};

const GRID_HZ: [(f64, &str); 3] = [(100.0, "100"), (1000.0, "1k"), (10_000.0, "10k")];
const GRID_DB: [f32; 3] = [6.0, 12.0, 18.0];

pub(crate) fn draw_space(list: &mut DrawList, inner: Rect, p: &Space, deepest_db: f32, signal_db: f32, spectrum: Option<&Spectrum>, cuts: &[f32; CURVE], caption: Option<(&Cell<Space>, &str)>) {
    let area = caption.map_or(plot(inner), |(cell, value)| captioned(list, inner, cell, value));
    let depth_y = |db: f32| area.y + (f64::from(area.h) * f64::from((db / GR_FULL_DB).clamp(0.0, 1.0))).round() as i32;
    for (hz, _) in GRID_HZ {
        list.fill(Rect::new(area.x + x_of_hz(hz, area.w), area.y, 1, area.h), Tone::Ink25);
    }
    for db in GRID_DB {
        list.fill(Rect::new(area.x, depth_y(db), area.w, 1), Tone::Ink25);
    }
    if let Some(spectrum) = spectrum.filter(|spectrum| spectrum.live()) {
        spectrum::draw(list, area, spectrum.pre(), Tone::Ink25);
    }
    for column in 0..area.w {
        let h = depth_y(cut_at(cuts, spectrum::hz(area.w, column))) - area.y;
        if h > 0 {
            list.fill(Rect::new(area.x + column, area.y, 1, h), Tone::Ink75);
            list.fill(Rect::new(area.x + column, area.y + h - 1, 1, 1), Tone::Ink100);
        }
    }
    let (low, high) = (area.x + x_of_hz(p.low_hz, area.w), area.x + x_of_hz(p.high_hz, area.w));
    list.fill(Rect::new(low, depth_y(p.depth_db as f32), high - low + 1, 1), Tone::Ink50);
    for (hz, label) in GRID_HZ {
        list.text(area.x + x_of_hz(hz, area.w) - LABEL_PAD - font::width(label, LABEL_SCALE), area.y + area.h - font::GLYPH_H - LABEL_PAD, label, Tone::Ink50, LABEL_SCALE);
    }
    for db in GRID_DB {
        list.text(area.x + LABEL_PAD, depth_y(db) + 1, &format!("-{db:.0}"), Tone::Ink50, LABEL_SCALE);
    }
    for x in [low, high] {
        list.fill(Rect::new(x, area.y, 1, area.h), Tone::Ink100);
    }
    list.fill(Rect::new(low, area.y, high - low + 1, 2), Tone::Ink100);
    let foot = area.y + area.h - font::GLYPH_H - LABEL_PAD;
    let low_label = spell(Hertz, p.low_hz);
    list.text(low + LABEL_PAD, foot, &low_label, Tone::Ink100, LABEL_SCALE);
    let high_label = spell(Hertz, p.high_hz);
    list.text(high - LABEL_PAD - font::width(&high_label, LABEL_SCALE), foot, &high_label, Tone::Ink100, LABEL_SCALE);
    let cut = if deepest_db < 0.05 { String::from("0.0 dB") } else { format!("-{deepest_db:.1} dB") };
    let signal = if signal_db <= -120.0 { String::from("SIGNAL -inf dB") } else { format!("SIGNAL {signal_db:+.1} dB") };
    let top = area.y + LABEL_PAD + 2;
    list.text(area.x + area.w - LABEL_PAD - font::width(&cut, LABEL_SCALE), top, &cut, Tone::Ink100, LABEL_SCALE);
    list.text(area.x + area.w - LABEL_PAD - font::width(&signal, LABEL_SCALE), top + font::GLYPH_H + 2, &signal, Tone::Ink75, LABEL_SCALE);
}

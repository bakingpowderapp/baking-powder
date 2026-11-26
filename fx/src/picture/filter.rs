use sdk::draw::{DrawList, Rect, Tone};
use sdk::face::{Cell, LABEL_SCALE, PITCH};
use sdk::font;
use crate::params::Filter;
use super::eq::{CURVE_FS, GRID_HZ, HANDLE, db_of_y, handle_at, hz_of_x, x_of_hz, y_of_db};
use super::{Drag, GRAB, LABEL_PAD, Picture, captioned, plot_over_caption};

const GRID_DB: [f64; 4] = [12.0, 0.0, -12.0, -24.0];

fn screen(inner: Rect) -> Rect {
    Rect::new(inner.x, inner.y, inner.w, inner.h - PITCH)
}

fn handle_db(filter: &Filter) -> f64 {
    if filter.on { filter.magnitude_db(filter.hz, CURVE_FS) } else { 0.0 }
}

fn handle_rect(plot: Rect, filter: &Filter) -> Rect {
    handle_at(plot, filter.hz, handle_db(filter))
}

fn sections(filter: &Filter) -> f64 {
    match filter.path() {
        sdk::dsp::filter::Path::Twos(count) => count as f64,
        _ => 1.0,
    }
}

pub(crate) fn draw_filter<P>(list: &mut DrawList, inner: Rect, site: &[(usize, Filter)], open: usize, cell: &Cell<P>, value: &str) {
    let plot = captioned(list, screen(inner), cell, value);
    let levels: Vec<i32> = (0..plot.w).map(|column| plot.y + y_of_db(site.iter().map(|(_, filter)| filter.magnitude_db(hz_of_x(column, plot.w), CURVE_FS)).sum(), plot.h)).collect();
    for (column, y) in (0..).zip(&levels) {
        list.fill(Rect::new(plot.x + column, *y, 1, plot.y + plot.h - y), Tone::Ink25);
    }
    for db in GRID_DB {
        list.fill(Rect::new(plot.x, plot.y + y_of_db(db, plot.h), plot.w, 1), Tone::Ink50);
    }
    for (hz, label) in GRID_HZ {
        let x = plot.x + x_of_hz(hz, plot.w);
        list.fill(Rect::new(x, plot.y, 1, plot.h), Tone::Ink50);
        list.text(x - LABEL_PAD - font::width(label, LABEL_SCALE), plot.y + plot.h - font::GLYPH_H - LABEL_PAD, label, Tone::Ink75, LABEL_SCALE);
    }
    let mut before = levels.first().copied().unwrap_or(plot.y);
    for (column, y) in (0..).zip(&levels) {
        let (top, bottom) = (before.min(*y), before.max(*y));
        list.fill(Rect::new(plot.x + column, top, 1, bottom - top + 1), Tone::Ink100);
        before = *y;
    }
    for (number, (unit, filter)) in (1..).zip(site) {
        let rect = handle_rect(plot, filter);
        let lit = *unit == open;
        list.fill(rect, if lit { Tone::Ink100 } else { Tone::Ground });
        list.frame(rect, 1, if filter.on || lit { Tone::Ink100 } else { Tone::Ink50 });
        list.text_centered(rect, &number.to_string(), if lit { Tone::Ground } else { Tone::Ink100 }, LABEL_SCALE);
    }
}

pub(super) fn drag(x: i32, y: i32, inner: Rect, filter: &mut Filter) -> bool {
    let plot = plot_over_caption(screen(inner));
    filter.hz = sdk::json::tidy(hz_of_x(x - plot.x, plot.w).clamp(Filter::MIN_HZ, Filter::MAX_HZ));
    if filter.peaked() {
        let db = db_of_y(y - plot.y, plot.h) / sections(filter);
        filter.q = sdk::json::tidy(10f64.powf(db / 20.0).clamp(Filter::MIN_Q, Filter::MAX_Q));
    }
    true
}

impl Picture {
    pub(crate) fn down_filter(&mut self, x: i32, y: i32, inner: Rect, site: &[(usize, Filter)]) -> Option<usize> {
        let plot = plot_over_caption(screen(inner));
        let near = |rect: &Rect| (rect.x + HANDLE / 2 - x).abs() + (rect.y + HANDLE / 2 - y).abs();
        let grabbed = site.iter().map(|(unit, filter)| (*unit, handle_rect(plot, filter))).filter(|(_, rect)| Rect::new(rect.x - GRAB / 2, rect.y - GRAB / 2, rect.w + GRAB, rect.h + GRAB).contains(x, y)).min_by_key(|(_, rect)| near(rect)).map(|(unit, _)| unit);
        self.origin = (x, y);
        self.armed = false;
        self.drag = Some(grabbed.map_or(Drag::Idle, |unit| Drag::Cut { unit }));
        grabbed
    }
}

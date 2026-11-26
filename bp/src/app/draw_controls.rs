use super::*;

const WARMING_SECONDS: f32 = 1.2;
const WARMING_SWEEP: i32 = 4;

impl App {
    pub(super) fn draw_fader(&mut self, inner: Rect, gain: f32, level: (f32, f32), clip: (bool, bool), gap: i32) {
        self.draw_levels(inner, level, clip, gap);
        self.draw_handle(inner, gain);
    }

    pub(super) fn draw_levels(&mut self, inner: Rect, level: (f32, f32), clip: (bool, bool), gap: i32) {
        let left = (inner.w - gap) / 2;
        let right = inner.w - left - gap;
        for (level, clip, x, w) in [(level.0, clip.0, inner.x, left), (level.1, clip.1, inner.x + left + gap, right)] {
            if level > 0.0 {
                let h = level_h(inner, level);
                self.list.fill(Rect::new(x, inner.y + inner.h - h, w, h), Tone::Ink50);
            }
            if clip {
                self.list.fill(Rect::new(x, inner.y, w, inner.h - level_h(inner, 1.0)), Tone::Ink75);
            }
        }
    }

    pub(super) fn draw_handle(&mut self, inner: Rect, gain: f32) {
        self.list.fill(Rect::new(inner.x, handle_y(inner, gain), inner.w, VOL_HANDLE_H), Tone::Ink100);
    }

    pub(super) fn draw_mark(&mut self, inner: Rect, mark: Mark, tone: Tone) {
        let (w, h) = mark.size();
        self.draw_mark_at(inner.x + (inner.w - w * TEXT_SCALE) / 2, inner.y + (inner.h - h * TEXT_SCALE) / 2, mark, TEXT_SCALE, tone);
    }

    pub(super) fn draw_mark_at(&mut self, x: i32, y: i32, mark: Mark, scale: i32, tone: Tone) {
        let list = &mut self.list;
        let mut dot = |dx: i32, dy: i32, w: i32, h: i32| list.fill(Rect::new(x + dx * scale, y + dy * scale, w * scale, h * scale), tone);
        match mark {
            Mark::Play => {
                for row in 0..11 {
                    let reach = if row < 5 { row } else { 10 - row };
                    dot(0, row, 2 * reach + 1, 1);
                }
            }
            Mark::Stop => dot(0, 0, 11, 11),
            Mark::Record => {
                for (row, inset) in [3, 2, 1, 1, 0, 0, 0, 1, 1, 2, 3].into_iter().enumerate() {
                    dot(inset, row as i32, 11 - 2 * inset, 1);
                }
            }
            Mark::Cycle => {
                for (dx, dy, w, h) in [(0, 1, 8, 1), (8, 0, 1, 3), (9, 1, 1, 1), (2, 6, 8, 1), (1, 5, 1, 3), (0, 6, 1, 1)] {
                    dot(dx, dy, w, h);
                }
            }
            Mark::Up => {
                dot(4, 0, 1, 9);
                for step in 0..4 {
                    dot(1 + step, 3 - step, 1, 1);
                    dot(7 - step, 3 - step, 1, 1);
                }
            }
            Mark::Down => {
                dot(4, 0, 1, 9);
                for step in 0..4 {
                    dot(1 + step, 5 + step, 1, 1);
                    dot(7 - step, 5 + step, 1, 1);
                }
            }
            Mark::Right => {
                dot(0, 4, 9, 1);
                for step in 0..4 {
                    dot(5 + step, 1 + step, 1, 1);
                    dot(5 + step, 7 - step, 1, 1);
                }
            }
            Mark::Metronome => {
                for row in 0..10 {
                    let reach = 2 + row * 4 / 9;
                    dot(6 - reach, row, 1, 1);
                    dot(6 + reach, row, 1, 1);
                }
                dot(6, 1, 1, 8);
                dot(0, 10, 13, 2);
            }
            Mark::Keyboard => {
                for (dx, dy, w, h) in [(0, 0, 16, 1), (0, 9, 16, 1), (0, 1, 1, 8), (15, 1, 1, 8)] {
                    dot(dx, dy, w, h);
                }
                for key in 1..4 {
                    dot(key * 4, 1, 1, 5);
                }
            }
            Mark::Heart => {
                for (dx, dy, w, h) in [(1, 0, 2, 1), (6, 0, 2, 1), (0, 1, 1, 1), (2, 1, 7, 1), (0, 2, 9, 2), (1, 4, 7, 1), (2, 5, 5, 1), (3, 6, 3, 1), (4, 7, 1, 1)] {
                    dot(dx, dy, w, h);
                }
            }
            Mark::Chord => {
                for (dx, dy) in [(0, 0), (4, 0), (8, 0), (0, 4), (4, 4), (8, 4)] {
                    dot(dx, dy, 3, 3);
                }
            }
            Mark::Hold => {
                for (dx, dy, w, h) in [(5, 0, 3, 1), (2, 1, 2, 1), (9, 1, 2, 1), (1, 2, 1, 1), (11, 2, 1, 1), (0, 3, 1, 1), (12, 3, 1, 1), (5, 5, 3, 3)] {
                    dot(dx, dy, w, h);
                }
            }
            Mark::Automation => {
                dot(0, 6, 3, 3);
                dot(10, 0, 3, 3);
                for step in 0..7 {
                    dot(3 + step, 6 - step, 1, 2);
                }
            }
            Mark::Lock => {
                for (dx, dy, w, h) in [(3, 0, 5, 1), (2, 1, 1, 4), (8, 1, 1, 4), (0, 5, 11, 7)] {
                    dot(dx, dy, w, h);
                }
            }
            Mark::Group => {
                for (dx, dy, w, h) in [(0, 0, 2, 9), (0, 0, 4, 1), (0, 8, 4, 1), (5, 1, 8, 1), (5, 4, 8, 1), (5, 7, 8, 1)] {
                    dot(dx, dy, w, h);
                }
            }
            Mark::Robot => {
                for (dx, dy, w, h) in [(5, 0, 3, 1), (6, 1, 1, 1), (0, 2, 13, 1), (0, 3, 1, 7), (12, 3, 1, 7), (3, 4, 2, 2), (8, 4, 2, 2), (3, 8, 7, 1), (0, 10, 13, 1)] {
                    dot(dx, dy, w, h);
                }
            }
        }
    }

    pub(super) fn draw_mark_cell(&mut self, rect: Rect, mark: Mark, hover: bool, on: bool) {
        let inner = tile_inner(rect);
        let (wash, ink) = wash_and_ink(on, hover);
        if let Some(tone) = wash {
            self.list.fill(inner, tone);
        }
        self.list.frame(rect, BORDER, Tone::Ink100);
        self.draw_mark(inner, mark, ink);
    }

    pub(super) fn draw_meter(&mut self, rect: Rect, thickness: i32, name: &str, value: &str, fraction: f32, hover: bool, pressed: bool) {
        let inner = Rect::new(rect.x + thickness, rect.y + thickness, rect.w - 2 * thickness, rect.h - 2 * thickness);
        let filled = inner.x + (inner.w as f32 * fraction.clamp(0.0, 1.0)).round() as i32;
        if hover {
            self.list.fill(inner, Tone::Ink25);
        }
        self.list.fill(Rect::new(inner.x, inner.y, filled - inner.x, inner.h), Tone::Ink100);
        self.list.frame(rect, thickness, Tone::Ink100);
        let label = if pressed { value } else { name };
        let x = rect.x + (rect.w - font::width(label, TEXT_SCALE)) / 2;
        self.list.text_split(x, label_y(rect), label, filled, Tone::Ground, Tone::Ink100, TEXT_SCALE);
    }

    pub(super) fn draw_warming(&mut self, rect: Rect, thickness: i32, seconds: f32) {
        let inner = Rect::new(rect.x + thickness, rect.y + thickness, rect.w - 2 * thickness, rect.h - 2 * thickness);
        let sweep = inner.w / WARMING_SWEEP;
        let turn = (seconds / WARMING_SECONDS).fract();
        let there_and_back = 0.5 - 0.5 * (std::f32::consts::TAU * turn).cos();
        let x = inner.x + ((inner.w - sweep) as f32 * there_and_back).round() as i32;
        self.list.fill(Rect::new(x, inner.y, sweep, inner.h), Tone::Ink25);
        self.list.frame(rect, thickness, Tone::Ink100);
        let x = rect.x + (rect.w - font::width(WARMING, TEXT_SCALE)) / 2;
        self.list.text(x, label_y(rect), WARMING, Tone::Ink100, TEXT_SCALE);
    }

    pub(super) fn draw_rise(&mut self, rect: Rect, thickness: i32, label: &str, fraction: f32, hover: bool) {
        let inner = Rect::new(rect.x + thickness, rect.y + thickness, rect.w - 2 * thickness, rect.h - 2 * thickness);
        let bottom = inner.y + inner.h;
        let filled = bottom - (inner.h as f32 * fraction.clamp(0.0, 1.0)).round() as i32;
        if hover {
            self.list.fill(inner, Tone::Ink25);
        }
        self.list.fill(Rect::new(inner.x, filled, inner.w, bottom - filled), Tone::Ink100);
        self.list.frame(rect, thickness, Tone::Ink100);
        let x = rect.x + (rect.w - font::width(label, TEXT_SCALE)) / 2;
        self.list.text_band(x, label_y(rect), label, filled, bottom, Tone::Ground, Tone::Ink100, TEXT_SCALE);
    }

    pub(super) fn draw_lean(&mut self, rect: Rect, label: &str, lean: f32, hover: bool) {
        let half = (rect.w - 1) / 2;
        let centre = rect.x + half;
        if hover {
            self.list.fill(rect, Tone::Ink25);
        }
        self.list.fill(Rect::new(centre, rect.y, 1, rect.h), Tone::Ink25);
        let (x, y) = (rect.x + TEXT_PAD, label_y(rect));
        if lean == 0.0 {
            self.list.text(x, y, label, Tone::Ink100, TEXT_SCALE);
            return;
        }
        let reach = ((lean.abs() * half as f32).round() as i32).clamp(1, half);
        let (from, cap) = if lean > 0.0 { (centre, centre + reach) } else { (centre - reach, centre - reach) };
        self.list.fill(Rect::new(from, rect.y, reach + 1, rect.h), Tone::Ink75);
        self.list.fill(Rect::new(cap, rect.y, 1, rect.h), Tone::Ink100);
        self.list.text_between(x, y, label, from, from + reach + 1, Tone::Ground, Tone::Ink100, TEXT_SCALE);
    }

    pub(super) fn draw_control(&mut self, rect: Rect, thickness: i32, label: &str, left: bool, hover: bool, on: bool) {
        let inner = Rect::new(rect.x + thickness, rect.y + thickness, rect.w - 2 * thickness, rect.h - 2 * thickness);
        let (wash, ink) = wash_and_ink(on, hover);
        if let Some(tone) = wash {
            self.list.fill(inner, tone);
        }
        self.list.frame(rect, thickness, Tone::Ink100);
        if left {
            self.list.text(inner.x + HEADER_PAD + TEXT_PAD, label_y(inner), label, ink, TEXT_SCALE);
        } else {
            self.list.text_centered(inner, label, ink, TEXT_SCALE);
        }
    }
}

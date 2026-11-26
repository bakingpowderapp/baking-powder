use crate::font;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn contains(self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }

    pub fn intersect(self, other: Rect) -> Rect {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = (self.x + self.w).min(other.x + other.w);
        let bottom = (self.y + self.h).min(other.y + other.h);
        Rect::new(x, y, right - x, bottom - y)
    }

    pub fn overlaps(self, other: Rect) -> bool {
        self.x < other.x + other.w && other.x < self.x + self.w && self.y < other.y + other.h && other.y < self.y + self.h
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    Ground,
    Ink25,
    Ink50,
    Ink75,
    Ink100,
}

impl Tone {
    fn ink(self) -> f64 {
        match self {
            Tone::Ground => 0.0,
            Tone::Ink25 => 0.25,
            Tone::Ink50 => 0.5,
            Tone::Ink75 => 0.75,
            Tone::Ink100 => 1.0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Theme {
    pub dark: bool,
    pub contrast: i32,
}

impl Theme {
    pub fn rgba(self, fill: Fill) -> [f64; 4] {
        let (ground, ink) = if self.dark { (0.0, 1.0) } else { (1.0, 0.0) };
        let level = ground + (ink - ground) * fill.ink() * self.contrast as f64 / 100.0;
        [level, level, level, 1.0]
    }
}

const PERCENT: i64 = 100;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lens {
    pub percent: i32,
    pub pixels: i32,
}

impl Lens {
    pub fn point(self, unit: i32) -> f64 {
        let pixel = (i64::from(unit) * i64::from(self.percent * self.pixels) + PERCENT / 2).div_euclid(PERCENT);
        pixel as f64 / f64::from(self.pixels)
    }

    pub fn units(self, points: f64) -> f64 {
        points * PERCENT as f64 / f64::from(self.percent)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fill {
    pub rect: Rect,
    pub tone: Tone,
    pub toward: Tone,
    pub mix: u8,
}

impl Fill {
    fn ink(self) -> f64 {
        let (from, to) = (self.tone.ink(), self.toward.ink());
        from + (to - from) * f64::from(self.mix) / f64::from(u8::MAX)
    }
}

pub struct DrawList {
    fills: Vec<Fill>,
    clip: Option<Rect>,
    origin: (i32, i32),
    scale: f32,
}

impl Default for DrawList {
    fn default() -> DrawList {
        DrawList::new()
    }
}

impl DrawList {
    pub fn new() -> DrawList {
        DrawList { fills: Vec::with_capacity(1024), clip: None, origin: (0, 0), scale: 1.0 }
    }

    pub fn clear(&mut self) {
        self.fills.clear();
        self.clip = None;
        self.transform((0, 0), 1.0);
    }

    pub fn clip(&mut self, clip: Option<Rect>) {
        self.clip = clip;
    }

    pub fn transform(&mut self, origin: (i32, i32), scale: f32) {
        self.origin = origin;
        self.scale = scale;
    }

    fn map(&self, rect: Rect) -> Rect {
        let (ox, oy) = self.origin;
        let edge = |origin: i32, v: i32| (origin as f32 + v as f32 * self.scale).round() as i32;
        let (left, top) = (edge(ox, rect.x), edge(oy, rect.y));
        let (right, bottom) = (edge(ox, rect.x + rect.w), edge(oy, rect.y + rect.h));
        Rect::new(left, top, right - left, bottom - top)
    }

    pub fn fills(&self) -> &[Fill] {
        &self.fills
    }

    pub fn fill(&mut self, rect: Rect, tone: Tone) {
        self.fade(rect, tone, tone, 0.0);
    }

    pub fn fade(&mut self, rect: Rect, from: Tone, to: Tone, travel: f32) {
        let rect = self.map(rect);
        let rect = match self.clip {
            Some(clip) => rect.intersect(clip),
            None => rect,
        };
        if rect.w > 0 && rect.h > 0 {
            self.fills.push(Fill { rect, tone: from, toward: to, mix: (travel.clamp(0.0, 1.0) * f32::from(u8::MAX)).round() as u8 });
        }
    }

    pub fn frame(&mut self, rect: Rect, thickness: i32, tone: Tone) {
        let Rect { x, y, w, h } = rect;
        self.fill(Rect::new(x, y, w, thickness), tone);
        self.fill(Rect::new(x, y + h - thickness, w, thickness), tone);
        self.fill(Rect::new(x, y + thickness, thickness, h - 2 * thickness), tone);
        self.fill(Rect::new(x + w - thickness, y + thickness, thickness, h - 2 * thickness), tone);
    }

    pub fn divide(&mut self, a: Rect, b: Rect, thickness: i32) {
        let shared = if b.x >= a.x + a.w - thickness {
            let top = a.y.max(b.y) + thickness;
            let bottom = (a.y + a.h).min(b.y + b.h) - thickness;
            Rect::new(b.x, top, thickness, bottom - top)
        } else {
            let left = a.x.max(b.x) + thickness;
            let right = (a.x + a.w).min(b.x + b.w) - thickness;
            Rect::new(left, b.y, right - left, thickness)
        };
        self.fill(shared, Tone::Ground);
    }

    pub fn text(&mut self, x: i32, y: i32, text: &str, tone: Tone, scale: i32) {
        glyph_runs(x, y, text, scale, |left, right, top| self.fill(Rect::new(left, top, right - left, scale), tone));
    }

    pub fn text_centered(&mut self, rect: Rect, text: &str, tone: Tone, scale: i32) {
        let x = rect.x + (rect.w - font::width(text, scale)) / 2;
        let y = rect.y + (rect.h - font::GLYPH_H * scale) / 2;
        self.text(x, y, text, tone, scale);
    }

    pub fn text_split(&mut self, x: i32, y: i32, text: &str, split: i32, before: Tone, after: Tone, scale: i32) {
        glyph_runs(x, y, text, scale, |left, right, top| {
            let cut = split.clamp(left, right);
            self.fill(Rect::new(left, top, cut - left, scale), before);
            self.fill(Rect::new(cut, top, right - cut, scale), after);
        });
    }

    pub fn text_between(&mut self, x: i32, y: i32, text: &str, from: i32, to: i32, inside: Tone, outside: Tone, scale: i32) {
        glyph_runs(x, y, text, scale, |left, right, top| {
            let (start, end) = (from.clamp(left, right), to.clamp(left, right));
            self.fill(Rect::new(left, top, start - left, scale), outside);
            self.fill(Rect::new(start, top, end - start, scale), inside);
            self.fill(Rect::new(end, top, right - end, scale), outside);
        });
    }

    pub fn text_band(&mut self, x: i32, y: i32, text: &str, band_top: i32, band_bottom: i32, inside: Tone, outside: Tone, scale: i32) {
        glyph_runs(x, y, text, scale, |left, right, top| {
            let tone = if top >= band_top && top < band_bottom { inside } else { outside };
            self.fill(Rect::new(left, top, right - left, scale), tone);
        });
    }
}

fn glyph_runs(x: i32, y: i32, text: &str, scale: i32, mut emit: impl FnMut(i32, i32, i32)) {
    let mut pen = x;
    for c in text.chars() {
        if let Some(rows) = font::glyph_char(c) {
            for row in 0..font::GLYPH_H {
                let mut run = None;
                for col in 0..=font::GLYPH_W {
                    let on = col < font::GLYPH_W && font::pixel(rows, col, row);
                    match (on, run) {
                        (true, None) => run = Some(col),
                        (false, Some(start)) => {
                            emit(pen + start * scale, pen + col * scale, y + row * scale);
                            run = None;
                        }
                        _ => {}
                    }
                }
            }
        }
        pen += font::ADVANCE * scale;
    }
}

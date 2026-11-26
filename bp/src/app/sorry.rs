use super::*;

const SORRY_LINES: [&str; 3] = ["Sorry love", "your sanctuary exists", "on a bigger screen"];
const HEARTS: u32 = 10;
const SORRY_PAD: i32 = 6;
const SORRY_LEADING: i32 = 3;
const SORRY_SPEED: (f32, f32) = (96.0, 64.0);
pub(super) const MIN_W: i32 = 1024;
pub(super) const MIN_H: i32 = 640;
const SORRY_SCALE_MAX: i32 = 4;

struct Heart {
    x: f32,
    y: f32,
    scale: i32,
    rise: f32,
    sway: f32,
}

impl Heart {
    fn new(seed: u32) -> Heart {
        let unit = |salt: u32| {
            let hash = (seed * 8 + salt).wrapping_mul(0x9E37_79B9).rotate_left(13).wrapping_mul(0x85EB_CA6B);
            (hash >> 8) as f32 / 16_777_216.0
        };
        Heart { x: unit(1), y: unit(2), scale: 2 + (unit(3) * 4.0) as i32, rise: 0.05 + 0.07 * unit(4), sway: unit(5) * std::f32::consts::TAU }
    }
}

pub(super) struct Sorry {
    at: (f32, f32),
    velocity: (f32, f32),
    bounces: u32,
    time: f32,
    hearts: Vec<Heart>,
    born: u32,
}

impl Sorry {
    pub(super) fn new() -> Sorry {
        Sorry { at: (BORDER as f32, BORDER as f32), velocity: SORRY_SPEED, bounces: 0, time: 0.0, hearts: (0..HEARTS).map(Heart::new).collect(), born: 0 }
    }
}

fn sorry_size(scale: i32) -> (i32, i32) {
    let widest = SORRY_LINES.iter().map(|line| font::width(line, scale)).max().unwrap_or(0);
    let lines = SORRY_LINES.len() as i32;
    (widest + 2 * SORRY_PAD * scale, (lines * font::GLYPH_H + (lines - 1) * SORRY_LEADING + 2 * SORRY_PAD) * scale)
}

fn reflect(at: &mut f32, velocity: &mut f32, room: f32) -> bool {
    if room <= 0.0 {
        *at = 0.0;
        false
    } else if *at < 0.0 {
        *at = -*at;
        *velocity = velocity.abs();
        true
    } else if *at > room {
        *at = (2.0 * room - *at).max(0.0);
        *velocity = -velocity.abs();
        true
    } else {
        false
    }
}

impl App {
    pub(super) fn small(&self) -> bool {
        self.width > 0 && (self.width < MIN_W || self.height < MIN_H)
    }

    pub(super) fn sorry_scale(&self) -> i32 {
        (1..=SORRY_SCALE_MAX)
            .rev()
            .find(|&scale| {
                let (w, h) = sorry_size(scale);
                w <= self.width - 2 * BORDER && h <= self.height - 2 * BORDER
            })
            .unwrap_or(1)
    }

    pub(super) fn bounce(&mut self, seconds: f32) {
        let (w, h) = sorry_size(self.sorry_scale());
        let room = ((self.width - w) as f32, (self.height - h) as f32);
        let sorry = &mut self.sorry;
        sorry.time += seconds;
        sorry.at.0 += sorry.velocity.0 * seconds;
        sorry.at.1 += sorry.velocity.1 * seconds;
        let bounced = reflect(&mut sorry.at.0, &mut sorry.velocity.0, room.0) | reflect(&mut sorry.at.1, &mut sorry.velocity.1, room.1);
        sorry.bounces += u32::from(bounced);
        for heart in &mut sorry.hearts {
            heart.y -= heart.rise * seconds;
            if heart.y < -0.15 {
                sorry.born += 1;
                *heart = Heart { y: 1.15, ..Heart::new(HEARTS + sorry.born) };
            }
        }
    }

    pub(super) fn draw_hearts(&mut self) {
        let t = self.sorry.time;
        let (w, h) = (self.width as f32, self.height as f32);
        let hearts: Vec<(i32, i32, i32)> = self.sorry.hearts.iter().map(|heart| ((heart.x * (w - 9.0 * heart.scale as f32) + (t * 1.3 + heart.sway).sin() * 3.0 * heart.scale as f32).round() as i32, (heart.y * h).round() as i32, heart.scale)).collect();
        for (x, y, scale) in hearts {
            for (dx, dy) in [(-scale, 0), (scale, 0), (0, -scale), (0, scale)] {
                self.draw_mark_at(x + dx, y + dy, Mark::Heart, scale, Tone::Ground);
            }
            self.draw_mark_at(x, y, Mark::Heart, scale, Tone::Ink100);
        }
    }

    pub(super) fn draw_sorry(&mut self) {
        self.draw_hearts();
        let scale = self.sorry_scale();
        let (w, h) = sorry_size(scale);
        let rect = Rect::new(self.sorry.at.0.round() as i32, self.sorry.at.1.round() as i32, w, h);
        let (fill, ink) = match self.sorry.bounces % 3 {
            0 => (Tone::Ground, Tone::Ink100),
            1 => (Tone::Ink25, Tone::Ink100),
            _ => (Tone::Ink100, Tone::Ground),
        };
        self.list.fill(rect, fill);
        self.list.frame(rect, scale, Tone::Ink100);
        let mut y = rect.y + SORRY_PAD * scale;
        for line in SORRY_LINES {
            self.list.text_centered(Rect::new(rect.x, y, rect.w, font::GLYPH_H * scale), line, ink, scale);
            y += (font::GLYPH_H + SORRY_LEADING) * scale;
        }
    }
}

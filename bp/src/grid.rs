use sdk::division::Division;
use sdk::draw::{DrawList, Rect, Tone};
use crate::project::{BEATS_PER_BAR, Clip, File, TICKS_PER_BAR, TICKS_PER_BEAT, beats, ticks};
use crate::wav::Wave;

pub const SNAPS: [Division; 15] = [
    Division::Bar,
    Division::HalfDotted,
    Division::Half,
    Division::QuarterDotted,
    Division::Third,
    Division::Quarter,
    Division::EighthDotted,
    Division::Sixth,
    Division::Eighth,
    Division::SixteenthDotted,
    Division::Twelfth,
    Division::Sixteenth,
    Division::ThirtySecondDotted,
    Division::TwentyFourth,
    Division::ThirtySecond,
];
pub const SNAP_DEFAULT: Division = Division::Quarter;
pub const BAR_W: i32 = 120;
pub const BEAT_W: f64 = BAR_W as f64 / BEATS_PER_BAR as f64;
pub const BEAT_W_MIN: f64 = 4.0;
pub const BEAT_W_MAX: f64 = 120.0;
pub const PLAYHEAD_W: i32 = 2;
pub const ZOOM_PX_PER_DOUBLE: f64 = 72.0;
pub const PAN_DAMPING: f64 = 10.0;
pub const PHRASE_BARS: i32 = 4;
const LINE_W: i32 = 2;
const PHRASE_W: i32 = 4;
const TICK_H: i32 = 6;
const STEP_W: f64 = 6.0;
const CLIP_INSET: i32 = 4;
const CLIP_GRAB: i32 = 8;
const PAD: i32 = 4;
const MIN_PITCH_ROWS: i32 = 7;
const NOTE_GAP: i32 = 1;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Lane {
    pub rect: Rect,
    pub scroll: i32,
    pub beat_w: f64,
    pub snap: i32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Line {
    Phrase,
    Bar,
    Step,
}

pub fn step_of(snap: Division) -> i32 {
    ticks(snap.beats())
}

fn bar_below(tick: i32) -> i32 {
    tick.div_euclid(TICKS_PER_BAR) * TICKS_PER_BAR
}

pub fn line_below(tick: i32, step: i32) -> i32 {
    let bar = bar_below(tick);
    bar + (tick - bar) / step * step
}

pub fn line_after(line: i32, step: i32) -> i32 {
    (line + step).min(bar_below(line) + TICKS_PER_BAR)
}

pub fn line_above(tick: i32, step: i32) -> i32 {
    let below = line_below(tick, step);
    if below == tick { tick } else { line_after(below, step) }
}

pub fn nearest_line(tick: i32, step: i32) -> i32 {
    let below = line_below(tick, step);
    let above = line_after(below, step);
    if tick - below <= above - tick { below } else { above }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClipHit {
    Body,
    Start,
    End,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClipLook {
    Plain,
    Hover,
    Picked,
}

pub fn inset(beat_w: f64) -> i32 {
    CLIP_INSET.min((beat_w / 2.0) as i32)
}

fn grab(w: i32) -> i32 {
    CLIP_GRAB.min(w / 3)
}

impl Lane {
    pub fn x_of(self, tick: i32) -> i32 {
        self.x_of_beat(beats(tick))
    }

    pub fn x_of_beat(self, beat: f64) -> i32 {
        self.rect.x + (beat * self.beat_w).round() as i32 - self.scroll
    }

    pub fn beats_at(self, x: i32) -> f64 {
        f64::from(x - self.rect.x + self.scroll) / self.beat_w
    }

    fn tick_at(self, x: i32) -> i32 {
        (self.beats_at(x) * f64::from(TICKS_PER_BEAT)).floor() as i32
    }

    pub fn floored(self, x: i32) -> i32 {
        line_below(self.tick_at(x), self.snap)
    }

    pub fn snapped(self, x: i32) -> i32 {
        nearest_line(self.tick_at(x), self.snap)
    }

    pub fn free(self) -> Lane {
        Lane { snap: 1, ..self }
    }

    pub fn cell_end(self, tick: i32) -> i32 {
        line_after(tick, self.snap)
    }

    pub fn bar_at(self, x: i32) -> i32 {
        bar_below(self.tick_at(x))
    }

    fn shown_step(self) -> Option<i32> {
        [self.snap, TICKS_PER_BEAT].into_iter().find(|&step| beats(step) * self.beat_w >= STEP_W)
    }

    pub fn lines(self) -> Vec<(i32, Line)> {
        let step = self.shown_step();
        let (from, to) = (self.tick_at(self.rect.x).max(1), self.tick_at(self.rect.x + self.rect.w - 1));
        let mut lines = Vec::new();
        for bar in from.div_euclid(TICKS_PER_BAR)..=to.div_euclid(TICKS_PER_BAR) {
            let start = bar * TICKS_PER_BAR;
            lines.push((start, if bar % PHRASE_BARS == 0 { Line::Phrase } else { Line::Bar }));
            if let Some(step) = step {
                lines.extend((start + step..start + TICKS_PER_BAR).step_by(step as usize).map(|tick| (tick, Line::Step)));
            }
        }
        lines.retain(|(tick, _)| (from..=to).contains(tick));
        lines
    }

    pub fn clip_rect(self, start: i32, length: i32) -> Rect {
        let inset = inset(self.beat_w);
        let x = self.x_of(start) + inset;
        Rect::new(x, self.rect.y + CLIP_INSET, (self.x_of(start + length) - inset - x).max(1), self.rect.h - 2 * CLIP_INSET)
    }

    pub fn hit_clip(self, clips: &[Clip], x: i32) -> Option<(usize, ClipHit)> {
        clips.iter().enumerate().find_map(|(index, clip)| {
            let rect = self.clip_rect(clip.start, clip.length);
            if x < rect.x || x >= rect.x + rect.w {
                return None;
            }
            let grab = grab(rect.w);
            let hit = if x >= rect.x + rect.w - grab {
                ClipHit::End
            } else if x < rect.x + grab {
                ClipHit::Start
            } else {
                ClipHit::Body
            };
            Some((index, hit))
        })
    }

    pub fn swept_clips(self, clips: &[Clip], marquee: Rect) -> impl Iterator<Item = &Clip> {
        clips.iter().filter(move |clip| self.clip_rect(clip.start, clip.length).overlaps(marquee))
    }

    pub fn draw(self, list: &mut DrawList) {
        for (tick, line) in self.lines() {
            let x = self.x_of(tick);
            match line {
                Line::Phrase | Line::Bar => {
                    let w = if line == Line::Phrase { PHRASE_W } else { LINE_W };
                    list.fill(Rect::new(x - (w - LINE_W) / 2, self.rect.y, w, self.rect.h), Tone::Ink100);
                }
                Line::Step => {
                    list.fill(Rect::new(x, self.rect.y, LINE_W, TICK_H), Tone::Ink100);
                    list.fill(Rect::new(x, self.rect.y + self.rect.h - TICK_H, LINE_W, TICK_H), Tone::Ink100);
                }
            }
        }
    }

    pub fn draw_clip(self, list: &mut DrawList, clip: &Clip, start: i32, length: i32, look: ClipLook, handles: bool, range: Option<(u8, u8)>, wave: Option<&Wave>, bpm: f64) {
        let rect = self.clip_rect(start, length);
        let (block, ink) = match look {
            ClipLook::Plain => (Tone::Ink25, Tone::Ink100),
            ClipLook::Hover => (Tone::Ink50, Tone::Ink100),
            ClipLook::Picked => (Tone::Ink100, Tone::Ground),
        };
        list.fill(rect, block);
        let notes = Rect::new(rect.x + PAD, rect.y + PAD, rect.w - 2 * PAD, rect.h - 2 * PAD);
        if let (Some(file), Some(wave)) = (&clip.file, wave) {
            self.draw_wave(list, Rect::new(rect.x, notes.y, rect.w, notes.h), wave, file, start, length, bpm, ink);
        } else if let Some((lo, hi)) = [range, clip.pitch_range()].into_iter().flatten().reduce(|(lo, hi), (a, b)| (lo.min(a), hi.max(b))) {
            let rows = (i32::from(hi) - i32::from(lo) + 1).max(MIN_PITCH_ROWS);
            let bottom_pitch = i32::from(lo) - (rows - (i32::from(hi) - i32::from(lo) + 1)) / 2;
            let row_h = notes.h as f32 / rows as f32;
            for note in &clip.notes {
                let x0 = self.x_of_beat(beats(start) + f64::from(note.start)).max(notes.x);
                let x1 = self.x_of_beat(beats(start) + f64::from((note.start + note.length).min(beats(length) as f32))).min(notes.x + notes.w);
                let row = rows - 1 - (i32::from(note.pitch) - bottom_pitch);
                let y = notes.y + (row as f32 * row_h).round() as i32;
                let h = ((row_h - NOTE_GAP as f32).round() as i32).max(1);
                list.fill(Rect::new(x0, y, (x1 - x0 - NOTE_GAP).max(1), h), ink);
            }
        }
        let grab = grab(rect.w);
        if handles && grab > LINE_W {
            for x in [rect.x + LINE_W, rect.x + rect.w - grab] {
                list.fill(Rect::new(x, rect.y + LINE_W, grab - LINE_W, rect.h - 2 * LINE_W), Tone::Ground);
            }
        }
    }

    fn draw_wave(self, list: &mut DrawList, area: Rect, wave: &Wave, file: &File, start: i32, length: i32, bpm: f64, ink: Tone) {
        let frames_per_beat = wave.rate * 60.0 / bpm * file.stretch(bpm);
        let (anchor, stop, gain) = (file.anchor(beats(start), bpm), file.stop(wave.frames(), wave.rate), file.gain());
        let frame_at = |x: i32| (self.beats_at(x) - anchor) * frames_per_beat;
        let (head, foot) = ((beats(start) - anchor) * frames_per_beat, (beats(start + length) - anchor) * frames_per_beat);
        let middle = area.y + area.h / 2;
        let (x0, x1) = (area.x.max(self.rect.x), (area.x + area.w).min(self.rect.x + self.rect.w));
        let mut run: Option<(i32, i32)> = None;
        for x in x0..=x1 {
            let height = if x == x1 {
                -1
            } else {
                let from = if x == area.x { head } else { frame_at(x) };
                let to = (if x + 1 == area.x + area.w { foot } else { frame_at(x + 1) }).min(stop);
                wave_height(wave.peak(from, to), gain * file.level_at((from + to) / 2.0 / wave.rate), area.h / 2)
            };
            match run {
                Some((_, ran)) if ran == height => {}
                Some((began, ran)) => {
                    if ran > 0 {
                        list.fill(Rect::new(began, middle - ran, x - began, 2 * ran), ink);
                    }
                    run = Some((x, height));
                }
                None => run = Some((x, height)),
            }
        }
    }
}

pub fn wave_height(peak: u8, gain: f64, half: i32) -> i32 {
    if peak == 0 || gain <= 0.0 {
        return 0;
    }
    ((f64::from(peak) / 255.0 * gain * f64::from(half)).round() as i32).clamp(1, half.max(1))
}

pub fn draw_ghost(list: &mut DrawList, at: (i32, i32), grab: i32, w: i32, h: i32) {
    let rect = Rect::new(at.0 - grab, at.1 - h / 2, w, h - 2 * CLIP_INSET);
    list.frame(rect, LINE_W, Tone::Ink25);
}

pub fn marquee(from: (i32, i32), to: (i32, i32)) -> Rect {
    let (x0, x1) = (from.0.min(to.0), from.0.max(to.0));
    let (y0, y1) = (from.1.min(to.1), from.1.max(to.1));
    Rect::new(x0, y0, (x1 - x0).max(1), (y1 - y0).max(1))
}

pub fn draw_marquee(list: &mut DrawList, marquee: Rect) {
    list.frame(marquee, LINE_W, Tone::Ink50);
}

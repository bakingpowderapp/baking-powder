use sdk::draw::{DrawList, Rect, Tone};
use sdk::font;
use crate::grid::{Lane, Line, PHRASE_BARS, PLAYHEAD_W, nearest_line};
use crate::project::{BEATS_PER_BAR, TICKS_PER_BAR, beats};

pub const H: i32 = 18;
const LINE_W: i32 = 2;
const BAR_W: i32 = 4;
const PHRASE_W: i32 = 6;
const GRAB: i32 = 8;
const ARM_PX: i32 = 4;
const MARK_ROWS: [i32; 4] = [14, 10, 6, 2];
const MARK_ROW_H: i32 = 2;
const HALO: i32 = 2;
const TICK_H: i32 = 8;
const STEP_H: i32 = 5;
const NUMBER_TOP: i32 = 1;
const NUMBER_GAP: i32 = 3;
const NUMBER_ROOM: i32 = 6;
const NUMBER_EVERY: [i32; 4] = [1, 4, 16, 64];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Loop {
    pub start: i32,
    pub end: i32,
}

impl Loop {
    pub fn bar(tick: i32) -> Loop {
        let start = tick.div_euclid(TICKS_PER_BAR) * TICKS_PER_BAR;
        Loop { start, end: start + TICKS_PER_BAR }
    }

    fn at(self, start: i32) -> Loop {
        Loop { start, end: start + self.end - self.start }
    }

    fn valid(self) -> bool {
        self.start >= 0 && self.end > self.start
    }

    pub fn contains(self, beat: f64) -> bool {
        beat >= beats(self.start) && beat < beats(self.end)
    }

    pub fn beats(self) -> (f64, f64) {
        (beats(self.start), beats(self.end))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hit {
    Cell(i32),
    Body,
    End,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Grab {
    Paint,
    Move(i32),
    End,
}

#[derive(Clone, Copy)]
struct Drag {
    grab: Grab,
    anchor: i32,
    held: Option<Loop>,
    origin: (i32, i32),
    armed: bool,
    lifted: bool,
}

pub enum Release {
    Click(i32),
    Edit,
}

#[derive(Clone, Copy)]
pub enum Down {
    Bar,
    Seek(i32),
}

pub struct Ruler {
    range: Option<Loop>,
    hover: Option<Hit>,
    drag: Option<Drag>,
}

impl Default for Ruler {
    fn default() -> Ruler {
        Ruler::new()
    }
}

impl Ruler {
    pub fn new() -> Ruler {
        Ruler { range: None, hover: None, drag: None }
    }

    pub fn range(&self) -> Option<Loop> {
        self.range
    }

    pub fn remember(&mut self, range: Option<Loop>) {
        self.range = range.filter(|range| range.valid());
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    fn lifted(&self) -> bool {
        self.drag.is_some_and(|drag| drag.lifted)
    }

    fn editing(&self) -> Option<Loop> {
        self.range.filter(|_| self.drag.is_some_and(|drag| drag.armed && !drag.lifted))
    }

    pub fn hit(&self, lane: Lane, x: i32) -> Hit {
        if let Some(range) = self.range {
            let rect = block(lane, range);
            if x >= rect.x && x < rect.x + rect.w {
                return if x >= rect.x + rect.w - GRAB.min(rect.w / 3) { Hit::End } else { Hit::Body };
            }
        }
        Hit::Cell(lane.floored(x).max(0))
    }

    pub fn hover(&mut self, hit: Option<Hit>) -> bool {
        let changed = hit != self.hover;
        self.hover = hit;
        changed
    }

    pub fn down(&mut self, lane: Lane, x: i32, y: i32, clicks: i32) -> Option<Down> {
        let cell = lane.floored(x).max(0);
        if clicks >= 2 {
            self.range = Some(Loop::bar(cell));
            return Some(Down::Bar);
        }
        let grab = match (self.hit(lane, x), self.range) {
            (Hit::Body, Some(range)) => Grab::Move(cell - range.start),
            (Hit::End, Some(_)) => Grab::End,
            _ => Grab::Paint,
        };
        self.drag = Some(Drag { grab, anchor: cell, held: self.range, origin: (x, y), armed: false, lifted: false });
        (grab != Grab::Paint).then_some(Down::Seek(cell))
    }

    pub fn moved(&mut self, lane: Lane, zone: Rect, x: i32, y: i32) -> bool {
        let Some(mut drag) = self.drag else {
            return false;
        };
        let cell = lane.floored(x).max(0);
        let arming = !drag.armed;
        drag.armed |= (x - drag.origin.0).abs() >= ARM_PX || (y - drag.origin.1).abs() >= ARM_PX || cell != drag.anchor;
        if !drag.armed {
            return false;
        }
        let range = match (drag.grab, drag.held) {
            (Grab::Move(grab), Some(held)) => held.at(nearest_line((cell - grab).max(0), lane.snap)),
            (Grab::End, Some(held)) => Loop { start: held.start, end: lane.cell_end(cell.max(held.start)) },
            _ => Loop { start: drag.anchor.min(cell), end: lane.cell_end(drag.anchor.max(cell)) },
        };
        let lifted = matches!(drag.grab, Grab::Move(_)) && !zone.contains(x, y);
        let changed = arming || Some(range) != self.range || lifted != drag.lifted;
        drag.lifted = lifted;
        self.drag = Some(drag);
        self.range = Some(range);
        changed
    }

    pub fn up(&mut self) -> Option<Release> {
        let drag = self.drag.take()?;
        if !drag.armed {
            if drag.grab != Grab::Paint {
                return None;
            }
            self.range = None;
            return Some(Release::Click(drag.anchor));
        }
        if drag.lifted {
            self.range = None;
        }
        Some(Release::Edit)
    }

    pub fn draw(&self, list: &mut DrawList, lane: Lane, start: i32, end: i32, playhead: Option<f64>, numbered: bool) {
        let strip = lane.rect;
        list.fill(strip, Tone::Ground);
        list.clip(Some(strip));
        if let (Some(Hit::Cell(cell)), None) = (self.hover, self.drag) {
            list.fill(Rect::new(lane.x_of(cell), strip.y, lane.x_of(lane.cell_end(cell)) - lane.x_of(cell), strip.h), Tone::Ink25);
        }
        let lifted = self.lifted();
        if let Some(range) = self.range {
            let rect = block(lane, range);
            if lifted {
                list.frame(rect, LINE_W, Tone::Ink25);
            } else {
                list.fill(rect, Tone::Ink100);
            }
        }
        let (tick_h, step_h) = if numbered { (TICK_H, STEP_H) } else { (strip.h, strip.h) };
        for (tick, line) in lane.lines() {
            match line {
                Line::Phrase | Line::Bar => {
                    let w = if line == Line::Phrase { PHRASE_W } else { BAR_W };
                    list.fill(Rect::new(lane.x_of(tick) - (w - LINE_W) / 2, strip.y + strip.h - tick_h, w, tick_h), Tone::Ink100);
                }
                Line::Step => list.fill(Rect::new(lane.x_of(tick), strip.y + strip.h - step_h, LINE_W, step_h), Tone::Ink50),
            }
        }
        let solid = self.range.filter(|_| !lifted);
        if numbered {
            draw_numbers(list, lane, strip, solid.map(|range| block(lane, range)), end);
        }
        if let Some(beat) = playhead {
            let tone = if solid.is_some_and(|range| range.contains(beat)) { Tone::Ground } else { Tone::Ink100 };
            list.fill(Rect::new(lane.x_of_beat(beat), strip.y, PLAYHEAD_W, strip.h), tone);
        }
        let dark = solid.map(|range| block(lane, range)).map(|rect| (rect.x, rect.x + rect.w));
        draw_mark(list, lane.x_of(start), strip, dark, false);
        if end > 0 {
            draw_mark(list, lane.x_of(end), strip, dark, true);
        }
        list.clip(None);
    }

    pub fn draw_edges(&self, list: &mut DrawList, lane: Lane, band: Rect) {
        let Some(range) = self.editing() else {
            return;
        };
        list.clip(Some(band));
        for tick in [range.start, range.end] {
            list.fill(Rect::new(lane.x_of(tick) - HALO, band.y, LINE_W + 2 * HALO, band.h), Tone::Ink25);
        }
        list.clip(None);
    }
}

pub fn draw_cue(list: &mut DrawList, lane: Lane, band: Rect, tick: i32, h: i32, tone: Tone) {
    list.fill(Rect::new(lane.x_of(tick) - HALO, band.y + band.h - h, LINE_W + 2 * HALO, h).intersect(band), tone);
}

fn draw_numbers(list: &mut DrawList, lane: Lane, strip: Rect, dark: Option<Rect>, end: i32) {
    let bar_w = beats(TICKS_PER_BAR) * lane.beat_w;
    let bar_of = |x: i32| (lane.beats_at(x) / f64::from(BEATS_PER_BAR)).floor().max(0.0) as i32;
    let (first, last) = (bar_of(strip.x), bar_of(strip.x + strip.w));
    let room = font::width(&(last + 1).to_string(), 1) + PHRASE_W + NUMBER_GAP + NUMBER_ROOM;
    let Some(every) = NUMBER_EVERY.into_iter().find(|every| f64::from(*every) * bar_w >= f64::from(room)) else {
        return;
    };
    for bar in (first - first % every..=last).step_by(every as usize) {
        let tick = bar * TICKS_PER_BAR;
        if end > 0 && tick == end {
            continue;
        }
        let w = if bar % PHRASE_BARS == 0 { PHRASE_W } else { BAR_W };
        let (x, y, label) = (lane.x_of(tick) + (w + LINE_W) / 2 + NUMBER_GAP, strip.y + NUMBER_TOP, (bar + 1).to_string());
        match dark {
            Some(block) => list.text_between(x, y, &label, block.x, block.x + block.w, Tone::Ground, Tone::Ink100, 1),
            None => list.text(x, y, &label, Tone::Ink100, 1),
        }
    }
}

fn block(lane: Lane, range: Loop) -> Rect {
    Rect::new(lane.x_of(range.start), lane.rect.y, lane.x_of(range.end) - lane.x_of(range.start) + LINE_W, lane.rect.h)
}

fn draw_mark(list: &mut DrawList, x: i32, strip: Rect, dark: Option<(i32, i32)>, hanging: bool) {
    for (row, width) in MARK_ROWS.iter().enumerate() {
        let inset = (row as i32 + 1) * MARK_ROW_H;
        let top = if hanging { strip.y + inset - MARK_ROW_H } else { strip.y + strip.h - inset };
        let rect = Rect::new(x + PLAYHEAD_W / 2 - width / 2, top, *width, MARK_ROW_H);
        fill_over(list, rect, dark);
    }
}

fn fill_over(list: &mut DrawList, rect: Rect, dark: Option<(i32, i32)>) {
    let (left, right) = (rect.x, rect.x + rect.w);
    let Some((from, to)) = dark else {
        list.fill(rect, Tone::Ink100);
        return;
    };
    let (from, to) = (from.clamp(left, right), to.clamp(left, right));
    list.fill(Rect::new(left, rect.y, from - left, rect.h), Tone::Ink100);
    list.fill(Rect::new(from, rect.y, to - from, rect.h), Tone::Ground);
    list.fill(Rect::new(to, rect.y, right - to, rect.h), Tone::Ink100);
}

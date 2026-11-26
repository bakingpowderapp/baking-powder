use crate::arp::{self, Arp, Join, LANE_STEPS, Lane, Pattern, SHORT_STEPS, Style};
use sdk::draw::{DrawList, Rect, Tone};
use sdk::face::{self, FRAME, ICON_NOTES, ICON_STEPS, LABEL_SCALE, PITCH};
use sdk::font;

pub const H: i32 = face::H;
const KEYS: i32 = 2 * PITCH;
const GAP: i32 = 1;
const MARK_H: i32 = 2;
const REST_MARK: i32 = 11;
const DOT: i32 = 2;
const DOT_PITCH: i32 = 3;
const SLIDE_RISE: i32 = 3;
const SIZES: [usize; 3] = [SHORT_STEPS, 2 * SHORT_STEPS, LANE_STEPS];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Band {
    Steps,
    Rest,
    Tie,
    Lane(Lane),
}

impl Band {
    const ARP: [Band; 5] = [Band::Steps, Band::Lane(Lane::Note), Band::Rest, Band::Lane(Lane::Mod), Band::Lane(Lane::Octave)];
    const STEP: [Band; 9] = [Band::Steps, Band::Lane(Lane::Semitone), Band::Lane(Lane::Cents), Band::Rest, Band::Tie, Band::Lane(Lane::Velocity), Band::Lane(Lane::Gate), Band::Lane(Lane::Mod), Band::Lane(Lane::Octave)];

    fn all(stepped: bool) -> &'static [Band] {
        if stepped { &Band::STEP } else { &Band::ARP }
    }

    fn rows(self, stepped: bool) -> i32 {
        match self {
            Band::Lane(Lane::Note) => 7,
            Band::Lane(Lane::Semitone) => 4,
            Band::Lane(Lane::Octave) if stepped => 2,
            Band::Lane(Lane::Octave) => 3,
            Band::Steps | Band::Rest | Band::Tie | Band::Lane(_) => 1,
        }
    }

    fn hit(self, index: usize) -> Hit {
        match self {
            Band::Steps => Hit::Count(index),
            Band::Rest => Hit::Rest(index),
            Band::Tie => Hit::Tie(index),
            Band::Lane(lane) => Hit::Step(lane, index),
        }
    }

    fn label(self, arp: &Arp) -> String {
        match self {
            Band::Steps if arp.pattern.length() == 1 => String::from("1 STEP"),
            Band::Steps => format!("{} STEPS", arp.pattern.length()),
            Band::Rest => String::from("REST"),
            Band::Tie => String::from("TIE"),
            Band::Lane(Lane::Velocity) if arp.fixed_velocity => String::from("VEL FIX"),
            Band::Lane(lane) => String::from(Lane::LABELS[lane.index()]),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hit {
    Nothing,
    Label(Band),
    Count(usize),
    Rest(usize),
    Tie(usize),
    Step(Lane, usize),
}

#[derive(Clone, Copy)]
enum Drag {
    Count(usize),
    Rest(bool, usize),
    Tie(Join, usize),
    Step(Lane, usize),
    Clear(Lane, usize),
}

impl Drag {
    fn hit(self) -> Hit {
        match self {
            Drag::Count(index) => Hit::Count(index),
            Drag::Rest(_, index) => Hit::Rest(index),
            Drag::Tie(_, index) => Hit::Tie(index),
            Drag::Step(lane, index) | Drag::Clear(lane, index) => Hit::Step(lane, index),
        }
    }
}

pub enum Under {
    Length(usize),
    Count(usize),
    Rests(usize),
    Rest(usize, bool),
    Ties(usize),
    Tie(usize, Join),
    Velocity(bool),
    Lane(Lane),
    Step(Lane, usize, i8),
}

pub struct Lanes {
    w: i32,
    columns: i32,
    stepped: bool,
    shown: usize,
    hover: Hit,
    drag: Option<Drag>,
}

impl Default for Lanes {
    fn default() -> Lanes {
        Lanes::new()
    }
}

fn rows(lane: Lane) -> i32 {
    let (lo, hi) = lane.range();
    i32::from(hi) - i32::from(lo) + 1
}

fn top_of(plot: Rect, lane: Lane, value: i32) -> i32 {
    let (lo, _) = lane.range();
    plot.y + plot.h - (value - i32::from(lo) + 1) * plot.h / rows(lane)
}

fn value_at(plot: Rect, lane: Lane, y: i32) -> i8 {
    let (lo, hi) = lane.range();
    let up = (plot.y + plot.h - 1 - y).clamp(0, plot.h - 1);
    (i32::from(lo) + up * rows(lane) / plot.h).min(i32::from(hi)) as i8
}

fn wash(inside: bool, hovered: bool) -> Option<Tone> {
    match (inside, hovered) {
        (true, true) => Some(Tone::Ink50),
        (true, false) => Some(Tone::Ink25),
        (false, _) => None,
    }
}

fn draw_run(list: &mut DrawList, block: Rect, style: Style, ink: Tone) {
    let pitch = ((block.w - DOT) / (ICON_STEPS - 1)).min(DOT_PITCH);
    if pitch < 1 {
        return;
    }
    let (w, h) = ((ICON_STEPS - 1) * pitch + DOT, (ICON_NOTES - 1) * DOT_PITCH + DOT);
    let (x, y) = (block.x + (block.w - w) / 2, block.y + (block.h - h) / 2);
    for step in 0..ICON_STEPS {
        let note = arp::order(style, i64::from(step), ICON_NOTES as usize) as i32;
        list.fill(Rect::new(x + step * pitch, y + (ICON_NOTES - 1 - note) * DOT_PITCH, DOT, DOT), ink);
    }
}

fn draw_join(list: &mut DrawList, cell: Rect, join: Join, ink: Tone) {
    let middle = cell.y + (cell.h - MARK_H) / 2;
    match join {
        Join::Play => {}
        Join::Tie => list.fill(Rect::new(cell.x - GAP, middle, cell.w + GAP, MARK_H), ink),
        Join::Slide => {
            let third = (cell.w + GAP) / 3;
            for part in 0..3 {
                list.fill(Rect::new(cell.x - GAP + part * third, middle + (1 - part) * SLIDE_RISE, if part == 2 { cell.w + GAP - 2 * third } else { third }, MARK_H), ink);
            }
        }
    }
}

impl Lanes {
    pub fn new() -> Lanes {
        Lanes { w: 0, columns: SHORT_STEPS as i32, stepped: false, shown: SHORT_STEPS, hover: Hit::Nothing, drag: None }
    }

    pub fn layout(&mut self, w: i32, arp: &Arp) {
        self.w = w;
        self.fit(arp);
    }

    fn fit(&mut self, arp: &Arp) {
        self.stepped = arp.stepped;
        let needed = self.shown.max(arp.pattern.length());
        self.columns = SIZES.into_iter().find(|size| *size >= needed).unwrap_or(LANE_STEPS) as i32;
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    fn left(&self, plot: Rect, index: i32) -> i32 {
        plot.x + index * plot.w / self.columns
    }

    fn column(&self, plot: Rect, index: i32) -> Rect {
        let (x0, x1) = (self.left(plot, index), self.left(plot, index + 1));
        Rect::new(x0, plot.y, x1 - x0 - GAP, plot.h)
    }

    fn column_at(&self, plot: Rect, x: i32) -> usize {
        ((x - plot.x) * self.columns / plot.w).clamp(0, self.columns - 1) as usize
    }

    fn band(&self, band: Band) -> Rect {
        let first: i32 = Band::all(self.stepped).iter().take_while(|other| **other != band).map(|other| other.rows(self.stepped)).sum();
        Rect::new(0, first * PITCH, self.w, band.rows(self.stepped) * PITCH + FRAME)
    }

    fn label_cell(&self, band: Band) -> Rect {
        let rect = self.band(band);
        Rect::new(0, rect.y, KEYS + FRAME, band.rows(self.stepped) * PITCH + FRAME)
    }

    fn plot_cell(&self, band: Band) -> Rect {
        let rect = self.band(band);
        Rect::new(KEYS, rect.y, self.w - KEYS, rect.h)
    }

    fn plot(&self, band: Band) -> Rect {
        face::interior(self.plot_cell(band))
    }

    fn hit(&self, x: i32, y: i32) -> Hit {
        for &band in Band::all(self.stepped) {
            if self.label_cell(band).contains(x, y) {
                return Hit::Label(band);
            }
            let plot = self.plot(band);
            if plot.contains(x, y) {
                return band.hit(self.column_at(plot, x));
            }
        }
        Hit::Nothing
    }

    fn lit(&self) -> Hit {
        self.drag.map_or(self.hover, Drag::hit)
    }

    pub fn hover(&mut self, point: Option<(i32, i32)>) -> bool {
        let hit = point.map_or(Hit::Nothing, |(x, y)| self.hit(x, y));
        let changed = hit != self.hover;
        self.hover = hit;
        changed
    }

    pub fn under(&self, arp: &Arp) -> Option<Under> {
        let pattern = &arp.pattern;
        match self.lit() {
            Hit::Nothing => None,
            Hit::Label(Band::Steps) => Some(Under::Length(pattern.length())),
            Hit::Label(Band::Rest) => Some(Under::Rests(pattern.rests())),
            Hit::Label(Band::Tie) => Some(Under::Ties(pattern.joins())),
            Hit::Label(Band::Lane(Lane::Velocity)) => Some(Under::Velocity(arp.fixed_velocity)),
            Hit::Label(Band::Lane(lane)) => Some(Under::Lane(lane)),
            Hit::Count(index) => Some(Under::Count(index + 1)),
            Hit::Rest(index) => Some(Under::Rest(index, pattern.rested(index))),
            Hit::Tie(index) => Some(Under::Tie(index, pattern.join(index))),
            Hit::Step(lane, index) => Some(Under::Step(lane, index, pattern.get(lane, index))),
        }
    }

    pub fn down(&mut self, x: i32, y: i32, shift: bool, arp: &mut Arp) -> bool {
        self.fit(arp);
        let hit = self.hit(x, y);
        self.hover = hit;
        let pattern = &mut arp.pattern;
        match hit {
            Hit::Nothing => false,
            Hit::Label(Band::Steps) if shift => {
                pattern.set_length(SHORT_STEPS);
                self.shown = SHORT_STEPS;
                true
            }
            Hit::Label(Band::Steps) => {
                self.shown = SIZES.into_iter().find(|size| *size > self.columns as usize).unwrap_or(SHORT_STEPS);
                true
            }
            Hit::Label(Band::Rest) => {
                if shift {
                    pattern.clear_rests();
                }
                shift
            }
            Hit::Label(Band::Tie) => {
                if shift {
                    pattern.clear_joins();
                }
                shift
            }
            Hit::Label(Band::Lane(Lane::Velocity)) if !shift => {
                arp.fixed_velocity = !arp.fixed_velocity;
                true
            }
            Hit::Label(Band::Lane(lane)) => {
                if shift {
                    pattern.clear(lane);
                }
                shift
            }
            Hit::Count(index) => {
                pattern.set_length(index + 1);
                self.drag = Some(Drag::Count(index));
                true
            }
            Hit::Rest(index) => {
                let rested = !shift && !pattern.rested(index);
                pattern.set_rest(index, rested);
                self.drag = Some(Drag::Rest(rested, index));
                true
            }
            Hit::Tie(index) => {
                let join = if shift { Join::Play } else { pattern.join(index).next() };
                pattern.set_join(index, join);
                self.drag = Some(Drag::Tie(join, index));
                true
            }
            Hit::Step(lane, index) if shift => {
                pattern.set(lane, index, lane.initial());
                self.drag = Some(Drag::Clear(lane, index));
                true
            }
            Hit::Step(lane, index) => {
                pattern.set(lane, index, value_at(self.plot(Band::Lane(lane)), lane, y));
                self.drag = Some(Drag::Step(lane, index));
                true
            }
        }
    }

    pub fn moved(&mut self, x: i32, y: i32, pattern: &mut Pattern) -> bool {
        match self.drag {
            Some(Drag::Count(_)) => {
                let index = self.column_at(self.plot(Band::Steps), x);
                pattern.set_length(index + 1);
                self.drag = Some(Drag::Count(index));
                true
            }
            Some(Drag::Rest(rested, _)) => {
                let index = self.column_at(self.plot(Band::Rest), x);
                pattern.set_rest(index, rested);
                self.drag = Some(Drag::Rest(rested, index));
                true
            }
            Some(Drag::Tie(join, _)) => {
                let index = self.column_at(self.plot(Band::Tie), x);
                pattern.set_join(index, join);
                self.drag = Some(Drag::Tie(join, index));
                true
            }
            Some(Drag::Step(lane, _)) => {
                let plot = self.plot(Band::Lane(lane));
                let index = self.column_at(plot, x);
                pattern.set(lane, index, value_at(plot, lane, y));
                self.drag = Some(Drag::Step(lane, index));
                true
            }
            Some(Drag::Clear(lane, _)) => {
                let index = self.column_at(self.plot(Band::Lane(lane)), x);
                pattern.set(lane, index, lane.initial());
                self.drag = Some(Drag::Clear(lane, index));
                true
            }
            None => false,
        }
    }

    pub fn up(&mut self) -> bool {
        self.drag.take().is_some()
    }

    fn draw_counts(&self, list: &mut DrawList, plot: Rect, length: i32, lit: Hit) {
        let room = font::width(&self.columns.to_string(), LABEL_SCALE) + GAP;
        let every = [1, 2, 4].into_iter().find(|every| every * plot.w / self.columns >= room).unwrap_or(4);
        for index in 0..self.columns {
            let wash = match (index < length, lit == Hit::Count(index as usize)) {
                (true, true) => Some(Tone::Ink75),
                (true, false) => Some(Tone::Ink100),
                (false, true) => Some(Tone::Ink25),
                (false, false) => None,
            };
            if let Some(wash) = wash {
                list.fill(self.column(plot, index), wash);
            }
        }
        for index in (0..self.columns).step_by(every as usize) {
            let (cell, label) = (self.column(plot, index), (index + 1).to_string());
            let outside = if lit == Hit::Count(index as usize) { Tone::Ink100 } else { Tone::Ink50 };
            list.text_between(cell.x + (cell.w - font::width(&label, LABEL_SCALE)) / 2, cell.y + (cell.h - font::GLYPH_H * LABEL_SCALE) / 2, &label, plot.x, self.left(plot, length) - GAP, Tone::Ground, outside, LABEL_SCALE);
        }
    }

    fn draw_rests(&self, list: &mut DrawList, plot: Rect, pattern: &Pattern, length: i32, lit: Hit) {
        for index in 0..self.columns {
            let cell = self.column(plot, index);
            let inside = index < length;
            if let Some(wash) = wash(inside, lit == Hit::Rest(index as usize)) {
                list.fill(cell, wash);
            }
            if pattern.rested(index as usize) {
                let size = REST_MARK.min(cell.w - 2 * GAP);
                let mark = Rect::new(cell.x + (cell.w - size) / 2, cell.y + (cell.h - size) / 2, size, size);
                list.fill(mark, if inside { Tone::Ink100 } else { Tone::Ink25 });
            }
        }
    }

    fn draw_ties(&self, list: &mut DrawList, plot: Rect, pattern: &Pattern, length: i32, lit: Hit) {
        for index in 0..self.columns {
            let cell = self.column(plot, index);
            let inside = index < length;
            if let Some(wash) = wash(inside, lit == Hit::Tie(index as usize)) {
                list.fill(cell, wash);
            }
            draw_join(list, cell, pattern.join(index as usize), if inside && !pattern.rested(index as usize) { Tone::Ink100 } else { Tone::Ink50 });
        }
    }

    fn draw_lane(&self, list: &mut DrawList, plot: Rect, pattern: &Pattern, lane: Lane, style: Style, length: i32, lit: Hit, playing: Option<i32>) {
        for index in 0..self.columns {
            if let Some(wash) = wash(index < length, lit == Hit::Step(lane, index as usize)) {
                list.fill(self.column(plot, index), wash);
            }
        }
        let (lo, hi) = lane.range();
        if lane == Lane::Note {
            for value in i32::from(lo)..i32::from(hi) {
                list.fill(Rect::new(plot.x, top_of(plot, lane, value), self.left(plot, length) - plot.x, GAP), Tone::Ground);
            }
        }
        let centred = lo < 0;
        let axis = if centred { (top_of(plot, lane, 0) + top_of(plot, lane, -1)) / 2 } else { plot.y + plot.h };
        if centred {
            list.fill(Rect::new(plot.x, axis, plot.w, 1), Tone::Ink50);
        }
        for index in 0..self.columns {
            let cell = self.column(plot, index);
            let sounding = index < length && !pattern.rested(index as usize);
            let (ink, mark) = if sounding { (Tone::Ink100, Tone::Ground) } else { (Tone::Ink25, Tone::Ink75) };
            let value = i32::from(pattern.get(lane, index as usize));
            let (top, bottom) = (top_of(plot, lane, value), top_of(plot, lane, value - 1));
            let (x, w) = (cell.x + GAP, cell.w - 2 * GAP);
            if lane == Lane::Note {
                let block = Rect::new(x, top + GAP, w, bottom - top - 2 * GAP);
                list.fill(block, ink);
                if value == i32::from(arp::RUN) {
                    draw_run(list, block, style, mark);
                } else {
                    list.text_centered(block, &value.to_string(), mark, LABEL_SCALE);
                }
                continue;
            }
            let (y0, y1) = match value {
                0 if centred => continue,
                value if value > 0 || !centred => (top, axis),
                _ => (axis + 1, bottom),
            };
            if sounding {
                list.fill(Rect::new(x, y0, w, y1 - y0), ink);
            } else {
                list.fill(Rect::new(x, if value < 0 { y1 - MARK_H } else { y0 }, w, MARK_H), ink);
            }
        }
        if let Some(slot) = playing.filter(|slot| *slot < self.columns) {
            let cell = self.column(plot, slot);
            list.fill(Rect::new(cell.x, cell.y, cell.w, MARK_H), Tone::Ink100);
        }
    }

    pub fn draw(&self, list: &mut DrawList, arp: &Arp, playing: Option<i64>) {
        let pattern = &arp.pattern;
        let lit = self.lit();
        let length = pattern.length() as i32;
        let playing = playing.map(|step| pattern.slot(step) as i32);
        for &band in Band::all(self.stepped) {
            let lit_label = (band == Band::Steps && self.columns > SHORT_STEPS as i32) || (band == Band::Lane(Lane::Velocity) && arp.fixed_velocity);
            face::draw_word_cell(list, self.label_cell(band), &band.label(arp), lit_label, lit == Hit::Label(band));
            list.frame(self.plot_cell(band), FRAME, Tone::Ink100);
            let plot = self.plot(band);
            match band {
                Band::Steps => self.draw_counts(list, plot, length, lit),
                Band::Rest => self.draw_rests(list, plot, pattern, length, lit),
                Band::Tie => self.draw_ties(list, plot, pattern, length, lit),
                Band::Lane(lane) => self.draw_lane(list, plot, pattern, lane, arp.style, length, lit, playing),
            }
        }
    }
}

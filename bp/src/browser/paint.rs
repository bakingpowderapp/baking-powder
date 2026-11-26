use sdk::draw::{DrawList, Rect, Tone};
use sdk::face::{self, CELL, FRAME, PITCH};
use sdk::font;
use super::tree::{Body, Line, Node};
use super::{Bar, Browser, Cells, Ghost, Hit, ASIDE_SCALE, BIG, NAME_SCALE, PAD, PENDING, SEARCH};

const FOLD_W: i32 = 4;
const FOLD_H: i32 = 7;
const FOLD_SCALE: i32 = 2;
const FOLDED_ROWS: i32 = 3;
const FOLDED_GAP: i32 = 2;
const FOLDED_BAR: (i32, i32) = (10, 3);
const GLASS_MARK: [(i32, i32, i32); 15] = [(0, 2, 3), (1, 1, 1), (1, 5, 1), (2, 0, 1), (2, 6, 1), (3, 0, 1), (3, 6, 1), (4, 0, 1), (4, 6, 1), (5, 1, 1), (5, 5, 1), (6, 2, 3), (6, 6, 2), (7, 7, 2), (8, 8, 2)];
const GLASS_SIZE: (i32, i32) = (10, 9);
const REVEAL_MARK: [(i32, i32, i32); 10] = [(0, 2, 5), (1, 5, 2), (2, 4, 1), (2, 6, 1), (3, 3, 1), (3, 6, 1), (4, 2, 1), (4, 6, 1), (5, 1, 1), (6, 0, 1)];
const REVEAL_SIZE: (i32, i32) = (7, 7);
const COPY_MARK: [(i32, i32, i32); 16] = [(0, 2, 6), (1, 2, 1), (1, 7, 1), (2, 0, 6), (2, 7, 1), (3, 0, 1), (3, 5, 1), (3, 7, 1), (4, 0, 1), (4, 5, 1), (4, 7, 1), (5, 0, 1), (5, 5, 3), (6, 0, 1), (6, 5, 1), (7, 0, 6)];
const COPY_SIZE: (i32, i32) = (8, 8);
const HEART_MARK: [(i32, i32, i32); 12] = [(0, 2, 2), (0, 7, 2), (1, 1, 4), (1, 6, 4), (2, 0, 11), (3, 0, 11), (4, 0, 11), (5, 1, 9), (6, 2, 7), (7, 3, 5), (8, 4, 3), (9, 5, 1)];
const HEART_RIM: [(i32, i32, i32); 22] = [(0, 2, 2), (0, 7, 2), (1, 1, 1), (1, 4, 1), (1, 6, 1), (1, 9, 1), (2, 0, 1), (2, 5, 1), (2, 10, 1), (3, 0, 1), (3, 10, 1), (4, 0, 1), (4, 10, 1), (5, 1, 1), (5, 9, 1), (6, 2, 1), (6, 8, 1), (7, 3, 1), (7, 7, 1), (8, 4, 1), (8, 6, 1), (9, 5, 1)];
const HEART_SIZE: (i32, i32) = (11, 10);
const TARGET_MARK: [(i32, i32, i32); 25] = [(0, 3, 5), (1, 2, 1), (1, 8, 1), (2, 1, 1), (2, 9, 1), (3, 0, 1), (3, 4, 3), (3, 10, 1), (4, 0, 1), (4, 3, 5), (4, 10, 1), (5, 0, 1), (5, 3, 5), (5, 10, 1), (6, 0, 1), (6, 3, 5), (6, 10, 1), (7, 0, 1), (7, 4, 3), (7, 10, 1), (8, 1, 1), (8, 9, 1), (9, 2, 1), (9, 8, 1), (10, 3, 5)];
const TARGET_SIZE: (i32, i32) = (11, 11);
const REFRESH_MARK: [(i32, i32, i32); 18] = [(0, 3, 5), (1, 2, 1), (1, 8, 1), (1, 10, 1), (2, 1, 1), (2, 9, 2), (3, 0, 1), (3, 7, 4), (4, 0, 1), (6, 10, 1), (7, 0, 4), (7, 10, 1), (8, 0, 2), (8, 9, 1), (9, 0, 1), (9, 2, 1), (9, 8, 1), (10, 3, 5)];
const REFRESH_SIZE: (i32, i32) = (11, 11);
const MARK_SCALE: i32 = 2;
const WAVE_MARK: [i32; 5] = [2, 5, 7, 4, 2];
const WAVE_BAR: i32 = 2;
const WAVE_GAP: i32 = 2;
const SLIDER_MARK: [i32; 3] = [8, 3, 6];
const SLIDER_RAIL: i32 = 2;
const SLIDER_PITCH: i32 = 7;
const SLIDER_H: i32 = 14;
const SLIDER_HANDLE: (i32, i32) = (6, 3);
const CLIP_MARK: [(i32, i32, i32); 5] = [(0, 0, 7), (0, 9, 9), (1, 3, 10), (2, 0, 4), (2, 6, 12)];
const NOTE_MARK: [(i32, i32, i32); 4] = [(2, 0, 5), (1, 6, 4), (0, 11, 7), (2, 12, 6)];
const CLIP_LANES: i32 = 3;
const CLIP_W: i32 = 18;
const CLIP_H: i32 = 4;
const CLIP_GAP: i32 = 2;

impl Browser {
    pub fn draw(&self, list: &mut DrawList, playing: Option<&str>, big: bool) {
        self.draw_bar(list, playing.is_some(), big);
        let lines = self.shown();
        let adding = self.adding();
        let held = self.hold.as_ref().filter(|hold| hold.armed);
        for visible in 1..self.rows() {
            let index = self.scroll + visible - 1;
            let y = visible as i32 * PITCH;
            match lines.get(index) {
                Some(&line) if held.is_some_and(|hold| hold.path == line.node.path) => {
                    let cells = self.cells(line, y);
                    for cell in [Some(cells.mark), Some(cells.name), cells.remove, cells.grip, cells.heart, Some(cells.copy), Some(cells.square)].into_iter().flatten() {
                        list.frame(cell, FRAME, Tone::Ink100);
                    }
                }
                Some(&line) => self.draw_line(list, self.cells(line, y), line, index, false, playing == Some(line.node.path.as_str())),
                None if adding && index == lines.len() => face::draw_word_cell(list, Rect::new(0, y, self.w, CELL), "ADD FOLDER", false, self.hover == Hit::Add),
                None => list.frame(Rect::new(0, y, self.w, CELL), FRAME, Tone::Ink100),
            }
        }
        if let Some(hold) = held
            && let Some(&line) = lines.iter().find(|line| line.node.path == hold.path)
        {
            let lift = (hold.at - hold.grab).clamp(PITCH, (self.rows() as i32 - 1) * PITCH);
            list.fill(Rect::new(0, lift, self.w, CELL), Tone::Ground);
            self.draw_line(list, self.cells(line, lift), line, 0, true, false);
        }
    }

    fn draw_bar(&self, list: &mut DrawList, playing: bool, big: bool) {
        let Bar { mark, name, refresh, grow, target, square, heart } = self.bar();
        let (hovered, filtering) = (self.hover == Hit::Search, self.filtering());
        draw_heart(list, heart, self.hearting(), self.hearting(), self.hover == Hit::Hearts);
        if let Some(refresh) = refresh {
            let (inner, ink) = face::lit(list, refresh, false, self.hover == Hit::Refresh);
            draw_mark(list, inner, &REFRESH_MARK, REFRESH_SIZE, ink);
        }
        if let Some(grow) = grow {
            face::draw_word_cell(list, grow, BIG, big, self.hover == Hit::Big);
        }
        match target {
            Some(target) if playing => {
                let (inner, ink) = face::lit(list, target, false, self.hover == Hit::Locate);
                draw_mark(list, inner, &TARGET_MARK, TARGET_SIZE, ink);
            }
            Some(target) => list.frame(target, FRAME, Tone::Ink100),
            None => {}
        }
        if filtering {
            face::draw_word_cell(list, square, "X", false, self.hover == Hit::Clear);
        } else {
            let (inner, ink) = face::lit(list, square, false, self.hover == Hit::Collapse);
            draw_folded(list, inner, ink);
        }
        list.frame(mark, FRAME, Tone::Ink100);
        let inner = face::interior(mark);
        if filtering {
            list.fill(inner, Tone::Ink100);
        } else if hovered {
            list.fill(inner, Tone::Ink25);
        }
        draw_mark(list, inner, &GLASS_MARK, GLASS_SIZE, if filtering { Tone::Ground } else { Tone::Ink100 });
        let search = self.search.as_ref();
        let text = search.map_or("", |search| search.text.as_str());
        let aside = match self.found() {
            _ if !filtering => String::new(),
            Some(found) => found.to_string(),
            None => String::from(PENDING),
        };
        if search.is_some_and(|search| search.focused) {
            draw_name(list, name, "", &aside, Some(text), false, true, hovered);
        } else if text.is_empty() {
            draw_name(list, name, SEARCH, "", None, true, false, hovered);
        } else {
            draw_name(list, name, text, &aside, None, false, false, hovered);
        }
    }

    fn draw_line(&self, list: &mut DrawList, cells: Cells, line: Line, index: usize, held: bool, playing: bool) {
        let hover = if self.hold.as_ref().is_some_and(|hold| hold.armed) { Hit::Nothing } else { self.hover };
        let Cells { mark, name, remove, grip, heart, copy, square } = cells;
        if let Some(remove) = remove {
            face::draw_word_cell(list, remove, "X", false, hover == Hit::Remove(index));
        }
        let (inner, ink) = face::lit(list, square, false, hover == Hit::Reveal(index));
        draw_mark(list, inner, &REVEAL_MARK, REVEAL_SIZE, ink);
        let (inner, ink) = face::lit(list, copy, self.copied && hover == Hit::Copy(index), hover == Hit::Copy(index));
        draw_mark(list, inner, &COPY_MARK, COPY_SIZE, ink);
        if let Some(heart) = heart {
            draw_heart(list, heart, self.hearts.contains(&line.node.path), false, hover == Hit::Heart(index));
        }
        if let Some(grip) = grip {
            let (inner, ink) = face::lit(list, grip, false, held || hover == Hit::Grip(index));
            face::draw_grip(list, inner, ink);
        }
        let mut x = 0;
        while x < mark.x {
            list.frame(Rect::new(x, mark.y, CELL, CELL), FRAME, Tone::Ink100);
            x += PITCH;
        }
        let (inner, ink) = face::lit(list, mark, playing, false);
        match &line.node.body {
            Body::Folder(folder) => {
                if hover == Hit::Mark(index) {
                    list.fill(inner, Tone::Ink25);
                }
                draw_fold(list, inner, folder.open);
            }
            Body::Preset { .. } => draw_slider_mark(list, inner),
            Body::Project { .. } => draw_lane_mark(list, inner, &CLIP_MARK, ink),
            Body::Wave(_) => draw_wave_mark(list, inner, ink),
            Body::Midi { .. } => draw_lane_mark(list, inner, &NOTE_MARK, ink),
            Body::Unreadable => {}
        }
        let selected = self.selected.as_deref() == Some(line.node.path.as_str());
        let typed = self.rename.as_ref().filter(|_| selected).map(|rename| rename.text.as_str());
        let faded = matches!(line.node.body, Body::Unreadable);
        draw_name(list, name, &line.node.name, line.node.aside(), typed, faded, selected, hover == Hit::Name(index));
    }

    pub fn draw_ghost(&self, list: &mut DrawList) {
        let Some(grip) = self.grip.as_ref().filter(|grip| grip.armed) else {
            return;
        };
        let Some(node) = self.node(&grip.path) else {
            return;
        };
        draw_ghost(list, Rect::new(grip.at.0 - grip.grab.0, grip.at.1 - grip.grab.1, grip.w, CELL), node);
    }

    pub fn carry(&mut self) -> Option<(String, Ghost)> {
        let grip = self.grip.take_if(|grip| grip.armed)?;
        let node = self.node(&grip.path)?;
        let mut list = DrawList::new();
        draw_ghost(&mut list, Rect::new(0, 0, grip.w, CELL), node);
        self.hover = Hit::Nothing;
        Some((grip.path, Ghost { fills: list.fills().to_vec(), w: grip.w, h: CELL, grab: grip.grab }))
    }
}

fn draw_ghost(list: &mut DrawList, rect: Rect, node: &Node) {
    list.fill(rect, Tone::Ground);
    draw_name(list, rect, &node.name, node.aside(), None, false, false, false);
}

fn draw_wave_mark(list: &mut DrawList, inner: Rect, ink: Tone) {
    let w = WAVE_MARK.len() as i32 * WAVE_BAR + (WAVE_MARK.len() as i32 - 1) * WAVE_GAP;
    let (x, middle) = (inner.x + (inner.w - w) / 2, inner.y + inner.h / 2);
    for (bar, half) in WAVE_MARK.iter().enumerate() {
        list.fill(Rect::new(x + bar as i32 * (WAVE_BAR + WAVE_GAP), middle - half, WAVE_BAR, 2 * half), ink);
    }
}

fn draw_slider_mark(list: &mut DrawList, inner: Rect) {
    let (handle_w, handle_h) = SLIDER_HANDLE;
    let w = (SLIDER_MARK.len() as i32 - 1) * SLIDER_PITCH + handle_w;
    let (x, y) = (inner.x + (inner.w - w) / 2, inner.y + (inner.h - SLIDER_H) / 2);
    for (rail, top) in SLIDER_MARK.iter().enumerate() {
        let left = x + rail as i32 * SLIDER_PITCH;
        list.fill(Rect::new(left + (handle_w - SLIDER_RAIL) / 2, y, SLIDER_RAIL, SLIDER_H), Tone::Ink100);
        list.fill(Rect::new(left, y + top, handle_w, handle_h), Tone::Ink100);
    }
}

fn draw_lane_mark(list: &mut DrawList, inner: Rect, marks: &[(i32, i32, i32)], ink: Tone) {
    let h = CLIP_LANES * CLIP_H + (CLIP_LANES - 1) * CLIP_GAP;
    let (x, y) = (inner.x + (inner.w - CLIP_W) / 2, inner.y + (inner.h - h) / 2);
    for &(lane, left, w) in marks {
        list.fill(Rect::new(x + left, y + lane * (CLIP_H + CLIP_GAP), w, CLIP_H), ink);
    }
}

fn draw_heart(list: &mut DrawList, cell: Rect, full: bool, lit: bool, hovered: bool) {
    let (inner, ink) = face::lit(list, cell, lit, hovered);
    draw_mark(list, inner, if full { &HEART_MARK } else { &HEART_RIM }, HEART_SIZE, ink);
}

fn draw_mark(list: &mut DrawList, inner: Rect, mark: &[(i32, i32, i32)], (w, h): (i32, i32), tone: Tone) {
    let s = MARK_SCALE;
    let (x, y) = (inner.x + (inner.w - w * s) / 2, inner.y + (inner.h - h * s) / 2);
    for &(row, left, run) in mark {
        list.fill(Rect::new(x + left * s, y + row * s, run * s, s), tone);
    }
}

fn draw_fold(list: &mut DrawList, inner: Rect, open: bool) {
    let s = FOLD_SCALE;
    if open {
        let (w, h) = (FOLD_H * s, FOLD_W * s);
        let (x, y) = (inner.x + (inner.w - w) / 2, inner.y + (inner.h - h) / 2);
        for row in 0..FOLD_W {
            list.fill(Rect::new(x + row * s, y + row * s, (FOLD_H - 2 * row) * s, s), Tone::Ink100);
        }
    } else {
        let (w, h) = (FOLD_W * s, FOLD_H * s);
        draw_closed(list, inner.x + (inner.w - w) / 2, inner.y + (inner.h - h) / 2, s, Tone::Ink100);
    }
}

fn draw_closed(list: &mut DrawList, x: i32, y: i32, s: i32, ink: Tone) {
    for column in 0..FOLD_W {
        list.fill(Rect::new(x + column * s, y + column * s, s, (FOLD_H - 2 * column) * s), ink);
    }
}

fn draw_folded(list: &mut DrawList, inner: Rect, ink: Tone) {
    let (bar_w, bar_h) = FOLDED_BAR;
    let w = FOLD_W + FOLDED_GAP + bar_w;
    let h = FOLDED_ROWS * FOLD_H + (FOLDED_ROWS - 1) * FOLDED_GAP;
    let (x, y) = (inner.x + (inner.w - w) / 2, inner.y + (inner.h - h) / 2);
    for row in 0..FOLDED_ROWS {
        let top = y + row * (FOLD_H + FOLDED_GAP);
        draw_closed(list, x, top, 1, ink);
        list.fill(Rect::new(x + FOLD_W + FOLDED_GAP, top + (FOLD_H - bar_h) / 2, bar_w, bar_h), ink);
    }
}

fn draw_name(list: &mut DrawList, rect: Rect, name: &str, aside: &str, typed: Option<&str>, faded: bool, selected: bool, hovered: bool) {
    let (inner, ink) = face::lit(list, rect, selected, hovered);
    let aside = &font::shortened(aside, font::glyphs_in(inner.w / 2, ASIDE_SCALE));
    let ink = if faded && !selected { Tone::Ink50 } else { ink };
    let right = inner.x + inner.w - PAD;
    let aside_w = font::width(aside, ASIDE_SCALE);
    if aside_w > 0 {
        list.text(right - aside_w, inner.y + (inner.h - font::GLYPH_H * ASIDE_SCALE) / 2, aside, if selected { Tone::Ground } else { Tone::Ink50 }, ASIDE_SCALE);
    }
    let room = right - aside_w - if aside_w > 0 { PAD } else { 0 } - (inner.x + PAD);
    let (x, y) = (inner.x + PAD, inner.y + (inner.h - font::GLYPH_H * NAME_SCALE) / 2);
    match typed {
        Some(text) => {
            list.text(x, y, text, ink, NAME_SCALE);
            list.fill(Rect::new(x + text.chars().count() as i32 * font::ADVANCE * NAME_SCALE, y, NAME_SCALE, font::GLYPH_H * NAME_SCALE), ink);
        }
        None => list.text(x, y, &font::shortened(name, font::glyphs_in(room, NAME_SCALE)), ink, NAME_SCALE),
    }
}

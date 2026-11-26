use crate::arp::Join;
use sdk::draw::{DrawList, Rect, Tone};
use sdk::face::{self, CELL, FRAME, PITCH};
use sdk::font;
use crate::grid::{self, PLAYHEAD_W};
use crate::project::{BEATS_PER_BAR, Clip, Note, TICKS_PER_BAR, TICKS_PER_BEAT, beats};
use crate::theory::{Root, flat_spelled};
use super::{LOW, TOP, KEYS, READOUT_SCALE, GUTTER, KEY_X, BLACK_W, BLACK, FIELD, CHOP, DASH, GRAB, NOTE_CELLS, STEP_PX, Ghost, Hit, Drag, Roll, key, handle_w, shown, shifted, step_of};

fn black(pitch: u8) -> bool {
    BLACK[usize::from(pitch % 12)]
}

pub fn name(pitch: u8) -> String {
    flat_spelled(&format!("{}{}", Root::ALL[usize::from(pitch % 12)].name(), i32::from(pitch / 12) - 1))
}

fn edges(rect: Rect, thickness: i32) -> [Rect; 4] {
    let Rect { x, y, w, h } = rect;
    [Rect::new(x, y, w, thickness), Rect::new(x, y + h - thickness, w, thickness), Rect::new(x, y, thickness, h), Rect::new(x + w - thickness, y, thickness, h)]
}

fn framed(list: &mut DrawList, rect: Rect, bounds: Rect, thickness: i32, tone: Tone) {
    for edge in edges(rect, thickness) {
        list.fill(edge.intersect(bounds), tone);
    }
}

fn chop(list: &mut DrawList, rect: Rect, bounds: Rect) {
    let slit = if rect.w < 3 * FRAME { 1 } else { FRAME };
    let first = DASH.min((rect.w - slit) / 2);
    let last = (rect.w - slit - DASH / 2).max(first);
    for at in (first.max(1)..=last).step_by(CHOP as usize) {
        list.fill(Rect::new(rect.x + at, rect.y, slit, rect.h).intersect(bounds), Tone::Ground);
    }
}

fn block(list: &mut DrawList, rect: Rect, bounds: Rect, stray: bool, picked: bool, handles: Option<i32>) {
    list.fill(rect.intersect(bounds), if stray { Tone::Ink75 } else { Tone::Ink100 });
    let ring = if rect.h >= 3 * FRAME && rect.w >= 3 * FRAME { FRAME } else { 1 };
    let handles = handles.filter(|grab| *grab > ring && rect.h >= 3 * ring);
    if stray {
        let inset = handles.unwrap_or(0);
        chop(list, Rect::new(rect.x + inset, rect.y, rect.w - 2 * inset, rect.h), bounds);
    }
    if rect.h < 3 * ring || rect.w < 3 * ring {
        return;
    }
    if picked {
        framed(list, Rect::new(rect.x + ring, rect.y + ring, rect.w - 2 * ring, rect.h - 2 * ring), bounds, ring, Tone::Ground);
    }
    if let Some(grab) = handles {
        for x in [rect.x + ring, rect.x + rect.w - grab] {
            list.fill(Rect::new(x, rect.y + ring, grab - ring, rect.h - 2 * ring).intersect(bounds), Tone::Ground);
        }
    }
}

fn joined(list: &mut DrawList, rect: Rect, bounds: Rect, join: Join) {
    if join == Join::Play || rect.w < 3 * GRAB || rect.h < 5 * FRAME {
        return;
    }
    let (x, middle, third) = (rect.x + 2 * FRAME, rect.y + (rect.h - FRAME) / 2, GRAB / 3);
    match join {
        Join::Play => {}
        Join::Tie => list.fill(Rect::new(x, middle, GRAB, FRAME).intersect(bounds), Tone::Ground),
        Join::Slide => {
            for part in 0..3 {
                list.fill(Rect::new(x + part * third, middle + (1 - part) * FRAME, third, FRAME).intersect(bounds), Tone::Ground);
            }
        }
    }
}

impl Roll {
    pub fn draw(&self, list: &mut DrawList, clip: &Clip, beside: &[Note], playhead: Option<f32>, scale: &[i32; 7], fitting: bool) {
        let (w, length, frame) = (self.w, clip.beats(), self.frame());
        let rows = self.rows_rect();
        let grid = self.grid_rect();
        let hi = self.pitch_at(PITCH).unwrap_or(TOP);
        let lo = self.pitch_at(self.h - 1).unwrap_or(LOW);
        let zooming = self.hover == Hit::Zoom;
        let outside = |pitch: u8| !scale.contains(&i32::from(pitch % 12));
        for pitch in lo..=hi {
            let (y, h) = (self.row_y(pitch), self.row_h(pitch));
            let gutter = Rect::new(FRAME, y + frame, GUTTER, h - frame);
            let key_inner = Rect::new(KEY_X, y + frame, KEYS - KEY_X, h - frame);
            let cell_inner = Rect::new(KEYS + FRAME, y + frame, w - KEYS - 2 * FRAME, h - frame);
            let sounding = self.sounding == Some(pitch);
            let hovered = self.hover == Hit::Key(pitch);
            let outside = outside(pitch);
            if zooming {
                list.fill(gutter.intersect(rows), Tone::Ink25);
            }
            if outside {
                list.fill(cell_inner.intersect(rows), Tone::Ink25);
            }
            if black(pitch) {
                if outside {
                    list.fill(key_inner.intersect(rows), Tone::Ink25);
                }
                let tone = if sounding {
                    Tone::Ink50
                } else if hovered {
                    Tone::Ink75
                } else {
                    Tone::Ink100
                };
                list.fill(Rect::new(key_inner.x, key_inner.y, BLACK_W, key_inner.h).intersect(rows), tone);
            } else {
                let step = usize::from(outside) + if sounding { 2 } else if hovered { 1 } else { 0 };
                if step > 0 {
                    list.fill(key_inner.intersect(rows), FIELD[step]);
                }
            }
            if pitch % 12 == 0 {
                let text = name(pitch);
                list.text(gutter.x + (gutter.w - font::width(&text, 1)) / 2, gutter.y + (gutter.h - font::GLYPH_H) / 2, &text, Tone::Ink100, 1);
            }
        }
        for note in beside.iter().filter(|note| shown(note, length)) {
            list.fill(self.note_rect(note, length).intersect(grid), Tone::Ink50);
        }
        let hovered = match self.hover {
            Hit::Ghost(over) => self.ghosts.get(over).map(Ghost::hold),
            _ => None,
        };
        let breath = self.breath();
        for ghost in self.ghosts.iter().filter(|ghost| shown(&ghost.note, length)) {
            let rect = self.note_rect(&ghost.note, length);
            let lit = self.held == Some(ghost.hold()) || hovered == Some(ghost.hold());
            let (body, rim, lifted) = if lit { (Tone::Ink50, Tone::Ink75, Tone::Ink100) } else { (Tone::Ink25, Tone::Ink50, Tone::Ink75) };
            list.fill(rect.intersect(grid), body);
            let ring = if rect.h >= 3 * FRAME && rect.w >= 3 * FRAME { FRAME } else { 1 };
            if rect.h >= 3 * ring && rect.w >= 3 * ring {
                for edge in edges(rect, ring) {
                    list.fade(edge.intersect(grid), rim, lifted, breath);
                }
            }
        }
        let step = [step_of(self.snap), TICKS_PER_BEAT].into_iter().find(|&step| beats(step) as f32 * self.beat_w(length) >= STEP_PX);
        let fits_bars = step.is_none_or(|step| TICKS_PER_BAR % step == 0);
        if let Some(step) = step {
            let mut tick = step;
            while (beats(tick) as f32) < length {
                if !fits_bars || tick % TICKS_PER_BAR != 0 {
                    let w = if tick % TICKS_PER_BEAT == 0 { FRAME } else { 1 };
                    list.fill(Rect::new(self.x_of(beats(tick) as f32, length), PITCH, w, self.visible()).intersect(grid), Tone::Ink50);
                }
                tick += step;
            }
        }
        for pitch in lo..=hi {
            list.fill(Rect::new(0, self.row_y(pitch), w, frame).intersect(rows), Tone::Ink100);
        }
        list.fill(Rect::new(0, self.row_y(lo) + self.row_h(lo), w, frame).intersect(rows), Tone::Ink100);
        for x in [0, FRAME + GUTTER] {
            list.fill(Rect::new(x, PITCH, FRAME, self.visible()), Tone::Ink100);
        }
        for beat in (0..=length.floor() as i32).step_by(BEATS_PER_BAR as usize).filter(|&beat| fits_bars || beat == 0 || beat as f32 == length) {
            list.fill(Rect::new(self.x_of(beat as f32, length), PITCH, FRAME, self.visible()).intersect(grid), Tone::Ink100);
        }
        for note in clip.notes.iter().filter(|note| shown(note, length)) {
            let rect = self.note_rect(note, length);
            block(list, rect, grid, outside(note.pitch), self.picked.contains(&key(note)), self.bracketed(note).then(|| handle_w(rect.w)));
            joined(list, rect, grid, note.articulation.join);
        }
        if let Some(Drag::Move(shift)) = &self.drag
            && shift.armed
            && self.off(shift.at)
        {
            let (beats, rows) = self.shift_of(shift, clip);
            for note in &shift.notes {
                framed(list, self.note_rect(&shifted(note, beats, rows), length), grid, FRAME, Tone::Ink25);
            }
        }
        if let Some(Drag::Marquee(marquee)) = &self.drag
            && marquee.armed
        {
            framed(list, grid::marquee(marquee.origin, marquee.at), grid, FRAME, Tone::Ink50);
        }
        if let Some(beat) = playhead.filter(|beat| *beat >= 0.0 && *beat < length) {
            list.fill(Rect::new(self.x_of(beat, length), PITCH, PLAYHEAD_W, self.visible()).intersect(grid), Tone::Ink100);
        }
        let [(big, _), (save, _), (preview, _), (guide, _), (fit, _), (down, _), (up, _), (reverse, _), (halve, _), (double, _), (chords, _)] = self.header();
        let after = (NOTE_CELLS + 1) * KEYS;
        let readout = Rect::new(after, 0, (chords.x + FRAME - after).max(0), CELL);
        let inner = face::interior(readout);
        list.fill(inner, Tone::Ground);
        list.frame(readout, FRAME, Tone::Ink100);
        let text = match (&self.drag, self.sounding) {
            (Some(Drag::Zoom(_)), _) => self.keys_label(),
            (_, Some(pitch)) => name(pitch),
            _ => clip.label.clone(),
        };
        list.text_centered(inner, &font::shortened(&text, font::glyphs_in(inner.w, READOUT_SCALE)), Tone::Ink100, READOUT_SCALE);
        list.fill(face::interior(self.left_cell(0)), Tone::Ground);
        let label = if matches!(self.drag, Some(Drag::Time(_) | Drag::TimeAt(_))) { self.bars_label(length) } else { String::from("ZOOM") };
        face::draw_value_cell(list, self.left_cell(0), false, self.time_travel(length), &label, self.hover == Hit::Time);
        for (cell, hit) in self.note_cells() {
            list.fill(face::interior(cell), Tone::Ground);
            match hit {
                Hit::Dial(dial) => {
                    let value = self.picked_value(clip, dial);
                    let travel = value.map_or(if dial.centred() { 0.5 } else { 0.0 }, |value| dial.travel(value));
                    let dragged = matches!(self.drag, Some(Drag::Dial { dial: dragged, .. }) if dragged == dial);
                    let label = match value.filter(|_| dragged) {
                        Some(value) => face::spell(dial.format(), value),
                        None => String::from(dial.label()),
                    };
                    face::draw_value_cell(list, cell, dial.centred(), travel, &label, self.hover == hit);
                }
                _ => {
                    let join = self.picked_join(clip);
                    face::draw_word_cell(list, cell, join.map_or("TIE", Join::spell), join.is_some_and(|join| join != Join::Play), self.hover == hit);
                }
            }
        }
        for (cell, label, on, hit) in [(chords, "CHORDS", false, Hit::Chords), (double, "×2", false, Hit::Double), (halve, "/2", false, Hit::Halve), (reverse, "REV", false, Hit::Reverse), (up, "+OCT", false, Hit::Up), (down, "−OCT", false, Hit::Down), (fit, "FIT", fitting, Hit::Fit), (guide, "GUIDE", self.guide, Hit::Guide), (preview, "PREVIEW", self.preview, Hit::Preview), (save, "SAVE", false, Hit::Save), (big, "BIG", self.big, Hit::Big)] {
            list.fill(face::interior(cell), Tone::Ground);
            face::draw_word_cell(list, cell, label, on, self.hover == hit);
        }
    }
}

use crate::arp::Join;
use crate::project::{Clip, Note, TICKS_PER_BEAT};
use super::{LOW, TOP, PITCHES, TOUCH, Dial, Hit, NoteKey, Shift, Stretch, Drag, Roll, Side, key, least, quantized, reversed, shown, snap_after, snap_before, snap_round, shifted, stretched};

fn joins_after(notes: &[Note]) -> Vec<Join> {
    let mut joins = vec![Join::Play; notes.len()];
    let joined: Vec<usize> = (0..notes.len()).filter(|&index| notes[index].articulation.join != Join::Play).collect();
    let mut times: Vec<f32> = joined.iter().map(|&index| notes[index].start).collect();
    times.sort_by(f32::total_cmp);
    times.dedup_by(|later, earlier| (*later - *earlier).abs() < TOUCH);
    for time in times {
        let mut starting: Vec<usize> = joined.iter().copied().filter(|&index| (notes[index].start - time).abs() < TOUCH).collect();
        let mut ending: Vec<usize> = (0..notes.len()).filter(|&index| (notes[index].start + notes[index].length - time).abs() < TOUCH).collect();
        starting.sort_by_key(|&index| notes[index].pitch);
        ending.sort_by_key(|&index| notes[index].pitch);
        for (ender, starter) in ending.into_iter().zip(starting) {
            joins[ender] = notes[starter].articulation.join;
        }
    }
    joins
}

fn bounded(notes: &[Note], beats: f32, rows: i32, length: f32) -> (f32, i32) {
    let first = notes.iter().map(|note| note.start).fold(f32::INFINITY, f32::min);
    let last = notes.iter().map(|note| note.start + note.length).fold(0.0, f32::max);
    let lo = notes.iter().map(|note| i32::from(note.pitch)).min().unwrap_or(i32::from(LOW));
    let hi = notes.iter().map(|note| i32::from(note.pitch)).max().unwrap_or(i32::from(TOP));
    (beats.clamp(-first, (length - last).max(0.0)), rows.clamp((i32::from(LOW) - lo).min(0), (i32::from(TOP) - hi).max(0)))
}

fn within(notes: impl Iterator<Item = Note>, length: f32) -> Vec<Note> {
    notes.filter(|note| note.start < length).map(|note| Note { length: note.length.min(length - note.start), ..note }).collect()
}

fn lay(clip: &mut Clip, rest: &[Note], placed: &[Note]) {
    clip.notes = rest.to_vec();
    for note in placed {
        clip.place_note(*note);
    }
}

fn scale(clip: &mut Clip, factor: f32, length: i32) {
    for note in &mut clip.notes {
        note.start *= factor;
        note.length *= factor;
    }
    clip.length = length;
}

fn turned(clip: &Clip) -> Vec<(usize, Note)> {
    let beats = clip.beats();
    let joins = joins_after(&clip.notes);
    clip.notes.iter().enumerate().filter(|(_, note)| note.start < beats).map(|(index, note)| (index, reversed(note, joins[index], beats))).collect()
}

pub fn quantize(clip: &mut Clip, snap: f32, chosen: impl Fn(usize, &Note) -> bool) -> Option<Vec<Note>> {
    let beats = clip.beats();
    let (mut notes, mut rest) = (Vec::new(), Vec::new());
    for (index, note) in clip.notes.iter().enumerate() {
        if chosen(index, note) { notes.push(*note) } else { rest.push(*note) }
    }
    let snapped: Vec<Note> = notes.iter().map(|note| quantized(note, beats, snap)).collect();
    if snapped == notes {
        return None;
    }
    lay(clip, &rest, &snapped);
    Some(snapped)
}

pub fn chordify(clip: &mut Clip) -> Option<Vec<Note>> {
    let chords = clip.bar_chords();
    if chords.is_empty() || chords == clip.notes {
        return None;
    }
    lay(clip, &[], &chords);
    Some(chords)
}

pub fn double(clip: &mut Clip, room: i32) -> bool {
    if clip.length > room {
        return false;
    }
    scale(clip, 2.0, clip.length * 2);
    true
}

pub fn halve(clip: &mut Clip) -> bool {
    if clip.length < 2 * TICKS_PER_BEAT {
        return false;
    }
    scale(clip, 0.5, (clip.length + 1) / 2);
    true
}

pub fn reverse(clip: &mut Clip) -> bool {
    let notes: Vec<Note> = turned(clip).into_iter().map(|(_, note)| note).collect();
    if notes.is_empty() {
        return false;
    }
    lay(clip, &[], &notes);
    true
}

impl Roll {
    pub fn copy_picked(&self, clip: &Clip) -> Vec<Note> {
        clip.notes.iter().filter(|note| self.picked.contains(&key(note))).copied().collect()
    }

    pub fn duplicate_picked(&mut self, clip: &mut Clip) -> bool {
        let notes = self.copy_picked(clip);
        let first = notes.iter().map(|note| note.start).fold(f32::INFINITY, f32::min);
        let last = notes.iter().map(|note| note.start + note.length).fold(0.0, f32::max);
        let span = last - first;
        let copies = within(notes.iter().map(|note| Note { start: note.start + span, ..*note }), clip.beats());
        if self.drag.is_some() || copies.is_empty() {
            return false;
        }
        let rest = clip.notes.clone();
        self.lay(clip, &rest, &copies);
        true
    }

    pub fn paste(&mut self, clip: &mut Clip, notes: &[Note]) -> bool {
        let fitted = within(notes.iter().copied(), clip.beats());
        if self.drag.is_some() || fitted.is_empty() {
            return false;
        }
        let rest = clip.notes.clone();
        self.lay(clip, &rest, &fitted);
        true
    }

    pub fn join_picked(&mut self, clip: &mut Clip) -> bool {
        if self.drag.is_some() || self.picked.len() < 2 {
            return false;
        }
        let (mut notes, rest): (Vec<Note>, Vec<Note>) = clip.notes.iter().partition(|note| self.picked.contains(&key(note)));
        notes.sort_by(|a, b| a.pitch.cmp(&b.pitch).then(a.start.total_cmp(&b.start)));
        let count = notes.len();
        let mut joined: Vec<Note> = Vec::with_capacity(count);
        for note in notes {
            match joined.last_mut() {
                Some(last) if last.pitch == note.pitch => {
                    last.length = (note.start + note.length).max(last.start + last.length) - last.start;
                }
                _ => joined.push(note),
            }
        }
        if joined.len() == count {
            return false;
        }
        self.lay(clip, &rest, &joined);
        true
    }

    pub fn stretch_picked(&mut self, clip: &mut Clip, direction: i32) -> bool {
        if self.picked.is_empty() || self.drag.is_some() {
            return false;
        }
        let (notes, rest): (Vec<Note>, Vec<Note>) = clip.notes.iter().partition(|note| self.picked.contains(&key(note)));
        let Some(anchor) = notes.iter().min_by(|a, b| a.start.total_cmp(&b.start)) else {
            return false;
        };
        let end = anchor.start + anchor.length;
        let delta = if direction > 0 { snap_after(end, self.snap) - end } else { snap_before(end, self.snap) - end };
        let resized: Vec<Note> = notes.iter().map(|note| stretched(note, Side::End, delta, clip.beats(), self.snap)).collect();
        if resized == notes {
            return false;
        }
        self.lay(clip, &rest, &resized);
        true
    }

    pub fn quantize(&mut self, clip: &mut Clip) -> bool {
        if self.drag.is_some() {
            return false;
        }
        let (all, beats) = (self.picked.is_empty(), clip.beats());
        let Some(snapped) = quantize(clip, self.snap, |_, note| if all { shown(note, beats) } else { self.picked.contains(&key(note)) }) else {
            return false;
        };
        self.picked = if all { Vec::new() } else { snapped.iter().map(key).collect() };
        true
    }

    pub(super) fn shift_octave(&mut self, clip: &mut Clip, rows: i32) -> bool {
        if self.drag.is_some() || clip.notes.is_empty() || bounded(&clip.notes, 0.0, rows, clip.beats()).1 != rows {
            return false;
        }
        for note in &mut clip.notes {
            *note = shifted(note, 0.0, rows);
        }
        for (pitch, _) in &mut self.picked {
            *pitch = (i32::from(*pitch) + rows) as u8;
        }
        true
    }

    pub(super) fn chordify(&mut self, clip: &mut Clip) -> bool {
        if self.drag.is_some() {
            return false;
        }
        let Some(chords) = chordify(clip) else {
            return false;
        };
        self.picked = chords.iter().map(key).collect();
        true
    }

    pub(super) fn double(&mut self, clip: &mut Clip, room: i32) -> bool {
        if self.drag.is_some() || !double(clip, room) {
            return false;
        }
        self.scale_picked(2.0);
        true
    }

    pub(super) fn halve(&mut self, clip: &mut Clip) -> bool {
        if self.drag.is_some() || !halve(clip) {
            return false;
        }
        self.scale_picked(0.5);
        true
    }

    pub(super) fn reverse(&mut self, clip: &mut Clip) -> bool {
        if self.drag.is_some() {
            return false;
        }
        let picked: Vec<NoteKey> = turned(clip).iter().filter(|(index, _)| self.picked.contains(&key(&clip.notes[*index]))).map(|(_, note)| key(note)).collect();
        if !reverse(clip) {
            return false;
        }
        self.picked = picked;
        true
    }

    fn scale_picked(&mut self, factor: f32) {
        for (_, start) in &mut self.picked {
            *start *= factor;
        }
    }

    pub(super) fn set_picked(&mut self, clip: &mut Clip, mut edit: impl FnMut(&mut Note)) -> bool {
        if self.drag.is_some() {
            return false;
        }
        let mut changed = false;
        for note in clip.notes.iter_mut().filter(|note| self.picked.contains(&key(note))) {
            let before = *note;
            edit(note);
            changed |= *note != before;
        }
        changed
    }

    pub(super) fn cycle_join(&mut self, clip: &mut Clip, shift: bool) -> bool {
        let join = if shift { Join::Play } else { self.picked_join(clip).map_or(Join::Tie, Join::next) };
        self.set_picked(clip, |note| note.articulation.join = join)
    }

    pub(super) fn picked_join(&self, clip: &Clip) -> Option<Join> {
        let mut joins = clip.notes.iter().filter(|note| self.picked.contains(&key(note))).map(|note| note.articulation.join);
        let first = joins.next()?;
        joins.all(|join| join == first).then_some(first)
    }

    pub(super) fn picked_value(&self, clip: &Clip, dial: Dial) -> Option<f64> {
        let values: Vec<Option<f64>> = clip.notes.iter().filter(|note| self.picked.contains(&key(note))).map(|note| dial.of(note)).collect();
        if values.is_empty() || values.iter().all(Option::is_none) {
            return None;
        }
        Some(values.iter().map(|value| value.unwrap_or(0.0)).sum::<f64>() / values.len() as f64)
    }

    pub fn delete_picked(&mut self, clip: &mut Clip) -> bool {
        if self.picked.is_empty() {
            return false;
        }
        let picked = std::mem::take(&mut self.picked);
        clip.notes.retain(|note| !picked.contains(&key(note)));
        true
    }

    pub(super) fn lay(&mut self, clip: &mut Clip, rest: &[Note], placed: &[Note]) {
        lay(clip, rest, placed);
        self.picked = placed.iter().map(key).collect();
    }

    pub(super) fn shift_of(&self, shift: &Shift, clip: &Clip) -> (f32, i32) {
        let raw = (shift.at.0 - shift.origin.0) as f32 / self.beat_w(clip.beats());
        let beats = snap_round(shift.anchor.start + raw, self.snap) - shift.anchor.start;
        let row_h = self.span() as f32 / PITCHES as f32;
        let rows = ((shift.origin.1 - shift.at.1) as f32 / row_h).round() as i32;
        bounded(&shift.notes, beats, rows, clip.beats())
    }

    pub fn nudge(&mut self, clip: &mut Clip, direction: i32, rows: i32) -> bool {
        if self.picked.is_empty() || self.drag.is_some() {
            return false;
        }
        let (mut moved, rest): (Vec<Note>, Vec<Note>) = clip.notes.iter().partition(|note| self.picked.contains(&key(note)));
        let first = moved.iter().map(|note| note.start).fold(f32::INFINITY, f32::min);
        let beats = match direction.cmp(&0) {
            std::cmp::Ordering::Greater => snap_after(first, self.snap) - first,
            std::cmp::Ordering::Less => snap_before(first, self.snap) - first,
            std::cmp::Ordering::Equal => 0.0,
        };
        let (beats, rows) = bounded(&moved, beats, rows, clip.beats());
        if beats == 0.0 && rows == 0 {
            return false;
        }
        for note in &mut moved {
            *note = shifted(note, beats, rows);
        }
        self.sounding = moved.first().map(|note| note.pitch);
        self.lay(clip, &rest, &moved);
        true
    }

    pub(super) fn stretch_of(&self, stretch: &Stretch, x: i32, length: f32) -> f32 {
        let raw = (x - stretch.origin_x) as f32 / self.beat_w(length);
        let anchor = stretch.anchor;
        let least = least(&anchor, stretch.side, self.snap);
        match stretch.side {
            Side::End => {
                let room = (length - anchor.start).max(least);
                let end = snap_round(anchor.start + anchor.length + raw, self.snap);
                (end - anchor.start).clamp(least, room) - anchor.length
            }
            Side::Start => snap_round(anchor.start + raw, self.snap).clamp(0.0, (anchor.start + anchor.length - least).max(0.0)) - anchor.start,
        }
    }

    pub(super) fn bracketed(&self, note: &Note) -> bool {
        let note = key(note);
        match (&self.drag, self.hover) {
            (Some(Drag::Resize(_)), _) => self.picked.contains(&note),
            (None, Hit::Start(held) | Hit::End(held)) => note == held || (self.picked.contains(&held) && self.picked.contains(&note)),
            _ => false,
        }
    }
}

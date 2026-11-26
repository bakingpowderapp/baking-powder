use super::*;
use crate::project::held;

const JOIN_EPSILON: f64 = 1e-3;
const LEAD_STEPS: f64 = 256.0;
const LEAD_BLOCKS: usize = 4;

fn joining<'a>(notes: &'a [SongNote], note: &SongNote) -> Option<&'a SongNote> {
    let from = notes.partition_point(|other| other.start < note.end - JOIN_EPSILON);
    notes[from..].iter().take_while(|other| other.start <= note.end + JOIN_EPSILON).find(|other| other.articulation.join != Join::Play)
}

fn ends_at(notes: &[SongNote], note: &SongNote) -> f64 {
    joining(notes, note).map_or(note.end, |other| other.start)
}

fn under(notes: &[SongNote], cursor: usize, beat: f64) -> impl Iterator<Item = (SongNote, f64)> + '_ {
    notes[..cursor].iter().map(|note| (note, ends_at(notes, note))).filter(move |&(note, end)| end > beat || joining(notes, note).is_some_and(|next| next.start >= beat)).map(|(note, end)| (SongNote { articulation: Articulation { join: Join::Play, ..note.articulation }, ..*note }, end))
}

fn chords(notes: &[SongNote], before: f64) -> impl Iterator<Item = (f64, f64)> + '_ {
    let mut held = notes.iter().take_while(move |note| note.start < before).peekable();
    std::iter::from_fn(move || {
        let first = held.next()?;
        let mut reach = ends_at(notes, first);
        while let Some(note) = held.next_if(|note| note.start < reach) {
            reach = reach.max(ends_at(notes, note));
        }
        Some((first.start, reach))
    })
}

impl Renderer {
    pub(super) fn line(&self) -> Segment {
        Segment { frame: self.anchor_frame, beat: self.anchor, bpm: self.bpm, frames_per_beat: self.frames_per_beat, playing: true }
    }

    pub(super) fn window(&self, track: usize) -> Option<Window> {
        self.span.map(|span| Window::of(span, self.table.tracks.get(track).map_or((0, 0), TrackCell::shift)))
    }

    pub(super) fn frames_to_window_grid(&self) -> usize {
        if self.playing || self.span.is_none() {
            return usize::MAX;
        }
        let laned = |track: usize| self.curves.get(track).is_some_and(|curves| !curves.is_empty()) || self.lfos.get(track).is_some_and(|lfos| !lfos.is_empty());
        let open = (0..self.count).any(|track| laned(track) && self.window(track).is_some_and(|window| window.moved && window.to > self.frame));
        if open { self.frames_to_grid() } else { usize::MAX }
    }

    fn frames_to_grid(&self) -> usize {
        LANE_GRID - (self.frame - self.anchor_frame).rem_euclid(LANE_GRID as i64) as usize
    }

    pub(super) fn seek_cursors(&mut self) {
        self.cursors.clear();
        for notes in self.song.iter() {
            self.cursors.push(notes.partition_point(|note| note.start < self.position));
        }
    }

    pub(super) fn lane(&self) -> Lane {
        if self.playing { Lane { beats: self.position, seconds: self.position * 60.0 / self.bpm } } else { Lane { beats: self.rested + self.rest_beats, seconds: self.rested_seconds + self.rest_seconds } }
    }

    pub(super) fn rest(&mut self) {
        self.rested = self.position;
        self.rested_seconds = self.position * 60.0 / self.bpm;
        self.rest_beats = 0.0;
        self.rest_seconds = 0.0;
    }

    pub(super) fn restart_lanes(&mut self, track: usize) {
        let key = Key { frame: self.frame, drift: 0.0 };
        if let Some(slot) = self.keys.get_mut(track) {
            *slot = Some(key);
        }
        self.lanes_due = true;
    }

    pub(super) fn start_notes_under_playhead(&mut self) {
        if self.span.is_some() {
            return;
        }
        let (now, position, frames_per_beat) = (self.frame, self.position, self.frames_per_beat);
        for (track, notes) in self.song.iter().enumerate() {
            let cursor = self.cursors.get(track).copied().unwrap_or(0);
            let began = now - ((position - held(&notes[..cursor]).start) * frames_per_beat).round() as i64;
            for (note, end) in under(notes, cursor, position) {
                if self.active.len() == self.active.capacity() || self.active.iter().any(|active| active.track == track && active.pitch == note.pitch) {
                    continue;
                }
                each_struck(&self.table, self.count, track, now, note.velocity, |runner, stream, velocity| runner.note_on(note.pitch, velocity, note.articulation, stream, began));
                self.active.push(Active { track, pitch: note.pitch, end });
            }
        }
    }

    pub(super) fn release_active(&mut self) {
        let now = self.frame;
        let playing_on: [bool; TRACKS] = std::array::from_fn(|track| self.window(track).is_some_and(|window| window.moved && window.to > now));
        let (table, count) = (&self.table, self.count);
        self.active.retain(|active| {
            let kept = playing_on.get(active.track).copied().unwrap_or(false);
            if !kept {
                each_midi(table, count, active.track, now, |runner, stream| runner.note_off(active.pitch, stream));
            }
            kept
        });
    }

    fn open_window(&mut self, track: usize, from: i64) {
        let (now, frames_per_beat, beat) = (self.frame, self.frames_per_beat, self.line().beat_at(from));
        let Some(notes) = self.song.get(track) else {
            return;
        };
        let cursor = notes.partition_point(|note| note.start < beat);
        if let Some(slot) = self.cursors.get_mut(track) {
            *slot = cursor;
        }
        let began = from - ((beat - held(&notes[..cursor]).start) * frames_per_beat).round() as i64;
        for (note, end) in under(notes, cursor, beat) {
            if self.active.len() == self.active.capacity() || self.active.iter().any(|active| active.track == track && active.pitch == note.pitch) {
                continue;
            }
            if from == now {
                each_struck(&self.table, self.count, track, now, note.velocity, |runner, stream, velocity| runner.note_on(note.pitch, velocity, note.articulation, stream, began));
            } else {
                each_struck(&self.table, self.count, track, now, note.velocity, |runner, stream, velocity| runner.cue_on(note.pitch, velocity, note.articulation, from, stream));
            }
            self.active.push(Active { track, pitch: note.pitch, end });
        }
    }

    fn close_window(&mut self, track: usize, to: i64) {
        let mut index = 0;
        while index < self.active.len() {
            if self.active[index].track == track {
                let active = self.active.swap_remove(index);
                each_midi(&self.table, self.count, track, self.frame, |runner, stream| runner.cue_off(active.pitch, to, stream));
            } else {
                index += 1;
            }
        }
    }

    pub(super) fn resettle_notes(&mut self) {
        if !self.playing {
            return;
        }
        let position = self.position;
        let mut index = 0;
        while index < self.active.len() {
            let active = self.active[index];
            let kept = self.song.get(active.track).and_then(|notes| notes.iter().map(|note| (note, ends_at(notes, note))).find(|(note, end)| note.pitch == active.pitch && note.start < position && *end > position));
            match kept {
                Some((_, end)) => {
                    self.active[index].end = end;
                    index += 1;
                }
                None => {
                    self.active.swap_remove(index);
                    each_midi(&self.table, self.count, active.track, self.frame, |runner, stream| runner.note_off(active.pitch, stream));
                }
            }
        }
        self.start_notes_under_playhead();
    }

    pub(super) fn begin_segment(&mut self) {
        let segment = if self.playing { Segment { frame: self.anchor_frame, beat: self.anchor, bpm: self.bpm, frames_per_beat: self.frames_per_beat, playing: true } } else { Segment::stopped(self.frame) };
        self.table.segments.write(&mut self.mark, segment);
    }

    pub(super) fn seek(&mut self, beat: f64) {
        self.release_active();
        forget_onsets(&self.table, self.count);
        self.position = beat;
        self.anchor = beat;
        self.anchor_frame = self.frame;
        self.lanes_due = true;
        self.begin_segment();
        self.origin = beat;
        self.seek_cursors();
        self.keys = [None; TRACKS];
        let (frame, frames_per_beat) = (self.frame, self.frames_per_beat);
        for track in 0..self.count {
            each_runner(&self.table, self.count, track, |runner| runner.place(frame, beat, frames_per_beat));
        }
        self.lead();
        self.start_notes_under_playhead();
        self.publish();
    }

    fn lead(&mut self) {
        if self.span.is_some() {
            return;
        }
        let (now, position, frames_per_beat) = (self.frame, self.position, self.frames_per_beat);
        let zero = now as f64 - position * frames_per_beat;
        for track in 0..self.count {
            let Some(lead) = self.table.rewind(track, zero, self.bpm) else {
                continue;
            };
            if let Some(from) = self.lead_in(track, lead) {
                self.roll_to(track, from, lead);
            }
            each_runner(&self.table, self.count, track, |runner| runner.place(now, position, frames_per_beat));
        }
    }

    fn lead_in(&self, track: usize, lead: Lead) -> Option<f64> {
        let position = self.position;
        let (from, _) = chords(self.song.get(track)?, position).find(|(_, reach)| reach + lead.memory > position)?;
        Some(from.max(position - if lead.recalls { LEAD_STEPS * lead.step } else { lead.memory }))
    }

    fn roll_to(&mut self, track: usize, from: f64, lead: Lead) {
        let (now, position, frames_per_beat, count) = (self.frame, self.position, self.frames_per_beat, self.count);
        let (line, half_frame) = (self.line(), 0.5 / frames_per_beat);
        let clock = Clock { sample_rate: self.sample_rate, bpm: self.bpm, beat: None };
        let chunk = ((lead.step * frames_per_beat) as usize).clamp(self.max_frames, LEAD_BLOCKS * self.max_frames) as i64;
        let start = now + ((from - position) * frames_per_beat).round() as i64;
        let Renderer { song, table, active, .. } = self;
        let Some(notes) = song.get(track) else {
            return;
        };
        let mut cursor = notes.partition_point(|note| note.start < from);
        if let Some(last) = cursor.checked_sub(1) {
            let restart = if lead.each_note { notes[last].start } else { chords(notes, from).last().map_or(from, |(start, _)| start) };
            let began = start - ((from - restart) * frames_per_beat).round() as i64;
            for (note, end) in notes[..cursor].iter().map(|note| (note, ends_at(notes, note))).filter(|(_, end)| *end > from) {
                if active.len() == active.capacity() || active.iter().any(|active| active.track == track && active.pitch == note.pitch) {
                    continue;
                }
                each_struck(table, count, track, start, note.velocity, |runner, stream, velocity| runner.note_on(note.pitch, velocity, note.articulation, stream, began));
                active.push(Active { track, pitch: note.pitch, end });
            }
        }
        let mut at = start;
        while at < now {
            let end = (at + chunk).min(now);
            let (beat, limit) = (line.beat_at(at), line.beat_at(end));
            let inside = |time: f64| time < limit - half_frame;
            let frame_of = |time: f64| at + (((time - beat) * frames_per_beat).round() as i64).max(0);
            let mut index = 0;
            while index < active.len() {
                if active[index].track == track && inside(active[index].end) {
                    let gone = active.swap_remove(index);
                    each_midi(table, count, track, at, |runner, stream| runner.cue_off(gone.pitch, frame_of(gone.end), stream));
                } else {
                    index += 1;
                }
            }
            while let Some(note) = notes.get(cursor).filter(|note| inside(note.start)) {
                cursor += 1;
                let end = ends_at(notes, note);
                let lasts = !inside(end);
                if lasts && active.len() == active.capacity() {
                    continue;
                }
                let on = frame_of(note.start);
                each_struck(table, count, track, at, note.velocity, |runner, stream, velocity| runner.cue_on(note.pitch, velocity, note.articulation, on, stream));
                if lasts {
                    active.push(Active { track, pitch: note.pitch, end });
                } else {
                    let off = frame_of(end);
                    each_midi(table, count, track, at, |runner, stream| runner.cue_off(note.pitch, off, stream));
                }
            }
            table.roll(track, &Dispatch { frames: (end - at) as usize, now: at, frames_per_beat, transport: Some(Transport { beat }), clock, ..Dispatch::idle() });
            at = end;
        }
    }

    pub(super) fn publish(&self) {
        self.playhead.store(pack(self.position, self.wraps, self.plays), Ordering::Relaxed);
    }

    pub(super) fn frames_to_seek(&self) -> Option<usize> {
        if !self.playing {
            return None;
        }
        let (_, end) = self.range.filter(|(_, end)| self.position < end - BEAT_EPSILON)?;
        Some(((end - self.position) * self.frames_per_beat).round().max(1.0) as usize)
    }

    pub(super) fn frames_to_lane_grid(&self) -> usize {
        let waved = !self.lfos.iter().all(Vec::is_empty) || !self.return_lfos.iter().all(Vec::is_empty);
        let curved = !self.curves.iter().all(Vec::is_empty) || !self.return_curves.iter().all(Vec::is_empty);
        if !waved && !curved {
            return usize::MAX;
        }
        let grid = if waved || self.segment(self.reach).playing { self.frames_to_grid() } else { usize::MAX };
        grid.min(self.frames_to_node())
    }

    fn frames_to_node(&self) -> usize {
        let tracks = self.curves.iter().zip(&self.table.tracks).take(self.count).enumerate().flat_map(|(index, (curves, cell))| {
            let clocking = self.clocking_of(index);
            curves.iter().filter(|curve| curve.lfo.is_none()).map(move |curve| (curve, self.lag_of(cell, &curve.aim, clocking), clocking))
        });
        let returns = self.return_curves.iter().enumerate().flat_map(|(index, curves)| curves.iter().filter(|curve| curve.lfo.is_none()).map(move |curve| (curve, self.return_lag(index, &curve.aim), self.clocking())));
        tracks.chain(returns).filter(|&(_, back, _)| back > 0).filter_map(|(curve, back, clocking)| self.frames_to_cross(&curve.nodes, back, clocking)).min().unwrap_or(usize::MAX)
    }

    fn frames_to_cross(&self, nodes: &[Node], back: usize, clocking: lanes::Clocking) -> Option<usize> {
        let segment = self.segment_of(back, clocking);
        let beat = segment.beat_at(self.frame - back as i64);
        let next = nodes.get(nodes.partition_point(|node| beats(node.tick) <= beat))?;
        Some((((beats(next.tick) - beat) * segment.frames_per_beat).round() as usize).max(1))
    }

    pub(super) fn backs(&self, mut each: impl FnMut(usize)) {
        for cell in self.table.tracks.iter().take(self.count) {
            if let Some(strip) = peek(&cell.strip)
                && let Some(plan) = strip.plan.as_deref()
            {
                each(plan.latency);
                for row in &plan.rows {
                    each(row.upstream);
                    each(row.upstream + row.latency);
                }
            }
        }
        for cell in &self.table.sends {
            if let Some(ret) = grab(&cell.strip) {
                each(ret.plan.latency);
                for lag in &ret.plan.rows {
                    each(lag.upstream);
                    each(lag.upstream + lag.latency);
                }
            }
        }
        each(self.output);
        for lag in &self.master_rows {
            each(lag.upstream);
            each(lag.upstream + lag.latency);
        }
    }

    pub(super) fn frames_to_shift(&self) -> usize {
        let crossing = |back: usize| self.table.segments.after(self.mark, self.frame - back as i64).filter(|segment| segment.playing).map_or(usize::MAX, |segment| (segment.frame + back as i64 - self.frame) as usize);
        if crossing(self.reach) == usize::MAX {
            return usize::MAX;
        }
        let mut least = usize::MAX;
        self.backs(|back| least = least.min(crossing(back)));
        least
    }

    pub(super) fn advance(&mut self, frames: usize) {
        let (now, end) = (self.frame, self.frame + frames as i64);
        let windows: [Option<Window>; TRACKS] = std::array::from_fn(|track| self.window(track));
        if !self.playing && !windows.iter().flatten().any(|window| window.moved && window.to >= now) {
            return;
        }
        let frames_per_beat = self.frames_per_beat;
        let position = if self.playing { self.position } else { self.line().beat_at(now) };
        let block_end = self.anchor + (end - self.anchor_frame) as f64 / frames_per_beat;
        let half_frame = 0.5 / frames_per_beat;
        let limits: [Option<f64>; TRACKS] = std::array::from_fn(|track| match windows[track] {
            None => self.playing.then_some(block_end),
            Some(window) if window.waits(now, frames) || window.to < now || (!window.moved && !self.playing) => None,
            Some(window) => Some(if window.to < end { self.line().beat_at(window.to) } else { block_end }),
        });
        let inside = |track: usize, beat: f64| limits.get(track).copied().flatten().is_some_and(|limit| beat < limit - half_frame);
        let frame_of = |beat: f64| now + (((beat - position) * frames_per_beat).round() as i64).max(0);
        for (track, window) in windows.iter().enumerate() {
            if let Some(window) = window.filter(|window| (now..end).contains(&window.from)) {
                self.open_window(track, window.from);
            }
        }
        let mut index = 0;
        while index < self.active.len() {
            if inside(self.active[index].track, self.active[index].end) {
                let active = self.active.swap_remove(index);
                let off = frame_of(active.end);
                each_midi(&self.table, self.count, active.track, now, |runner, stream| runner.cue_off(active.pitch, off, stream));
            } else {
                index += 1;
            }
        }
        for (track, notes) in self.song.iter().enumerate() {
            let Some(cursor) = self.cursors.get_mut(track) else {
                continue;
            };
            while let Some(note) = notes.get(*cursor).filter(|note| inside(track, note.start)) {
                *cursor += 1;
                let end = ends_at(notes, note);
                let held = !inside(track, end);
                if held && self.active.len() == self.active.capacity() {
                    continue;
                }
                let on = frame_of(note.start);
                each_struck(&self.table, self.count, track, now, note.velocity, |runner, stream, velocity| runner.cue_on(note.pitch, velocity, note.articulation, on, stream));
                if held {
                    self.active.push(Active { track, pitch: note.pitch, end });
                } else {
                    let off = frame_of(end);
                    each_midi(&self.table, self.count, track, now, |runner, stream| runner.cue_off(note.pitch, off, stream));
                }
            }
        }
        for (track, window) in windows.iter().enumerate() {
            if let Some(window) = window.filter(|window| window.moved && (now..end).contains(&window.to)) {
                self.close_window(track, window.to);
            }
        }
        if self.playing {
            self.position = block_end;
            self.publish();
        }
    }
}

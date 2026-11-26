use super::*;

const CUT_FLASH_SECONDS: f32 = 0.15;

pub(super) const CUT_HOLD_SECONDS: f32 = 1.5;

#[derive(Clone, Copy)]
pub(super) struct ClipDrag {
    pub(super) track: usize,
    pub(super) index: usize,
    pub(super) hit: ClipHit,
    pub(super) grab: i32,
    pub(super) origin: (i32, i32),
    pub(super) at: (i32, i32),
    pub(super) armed: bool,
    pub(super) held: f32,
}

impl ClipDrag {
    pub(super) fn resizing(self) -> bool {
        self.hit != ClipHit::Body
    }
}

#[derive(Clone, Copy)]
pub(super) struct ChordDrag {
    pub(super) chord: Chord,
    pub(super) at: (i32, i32),
}

#[derive(Clone)]
pub(super) struct Marquee {
    pub(super) origin: (i32, i32),
    pub(super) at: (i32, i32),
    pub(super) armed: bool,
    pub(super) hit: Option<(usize, usize)>,
    pub(super) before: Vec<(usize, i32)>,
}

impl App {
    pub(super) fn clip_drag(&self) -> Option<ClipDrag> {
        match self.drag {
            Some(Drag::Clip(drag)) => Some(drag),
            _ => None,
        }
    }

    pub(super) fn unpick_elsewhere(&mut self) {
        self.face.unpick();
        self.rack.unpick();
        self.unpick_span();
        self.selection.paste_point = None;
        self.selection.region = None;
    }

    pub(super) fn pick_clip(&mut self, track: usize, index: usize) {
        if self.shift {
            return self.toggle_clip(track, index);
        }
        self.selection.paste_point = None;
        self.selection.clips = vec![(track, self.project.tracks[track].clips[index].start)];
    }

    pub(super) fn toggle_clip(&mut self, track: usize, index: usize) {
        self.selection.paste_point = None;
        let key = (track, self.project.tracks[track].clips[index].start);
        match self.selection.clips.iter().position(|picked| *picked == key) {
            Some(at) => {
                self.selection.clips.remove(at);
            }
            None => self.selection.clips.push(key),
        }
    }

    pub(super) fn insert_silence(&mut self) -> bool {
        let Some(range) = self.selection.region_range().or_else(|| self.ruler.range()) else {
            return false;
        };
        let (at, by) = (range.start, range.end - range.start);
        if self.reach_after(at).is_some_and(|reach| reach > LONGEST_TICKS - by) {
            return false;
        }
        self.project.insert_time(at, by);
        let shifted = |tick: i32| if tick >= at { tick + by } else { tick };
        for (_, start) in &mut self.selection.clips {
            *start = shifted(*start);
        }
        if let Some(region) = &mut self.selection.region {
            (region.start, region.end) = (region.start + by, region.end + by);
        }
        self.ruler.remember(self.ruler.range().map(|held| Loop { start: shifted(held.start), end: shifted(held.end) }));
        if self.play_from >= at {
            self.mark_start(self.play_from + by);
        }
        true
    }

    fn reach_after(&self, at: i32) -> Option<i32> {
        let ends = self.project.tracks.iter().flat_map(|track| &track.clips).map(Clip::end).filter(|end| *end > at);
        let lanes = self.project.tracks.iter().map(|track| &track.automation).chain(self.project.returns().map(|ret| &ret.automation));
        let nodes = lanes.flat_map(|lane| &lane.curves).flat_map(|curve| &curve.nodes).map(|node| node.tick).filter(|tick| *tick >= at);
        ends.chain(nodes).max()
    }

    pub(super) fn join_loop(&mut self) -> bool {
        let Some(range) = self.ruler.range() else {
            return false;
        };
        let tracks = self.selection.picked_tracks();
        self.join_between(range, tracks);
        true
    }

    pub(super) fn join_between(&mut self, range: Loop, tracks: Vec<usize>) {
        let bpm = self.project.bpm;
        let mut done = Vec::new();
        for track in tracks {
            let before = self.project.tracks[track].clips.clone();
            let parts = self.project.tracks[track].take_between(range.start, range.end, bpm);
            match self.joined_file(&parts, range.start, range.end) {
                Ok(file) => {
                    self.project.tracks[track].place(joined(&parts, range.start, range.end, file), bpm);
                    done.push(track);
                }
                Err(reason) => self.refuse_join(track, before, reason),
            }
        }
        done.sort_by_key(|track| *track == self.selection.track);
        self.selection.clips.extend(done.into_iter().map(|track| (track, range.start)));
    }

    pub(super) fn join_selected_clips(&mut self) -> bool {
        let (mut along, mut bounced) = (false, false);
        for track in 0..self.project.tracks.len() {
            let starts: Vec<i32> = self.selection.clips.iter().filter(|(t, _)| *t == track).map(|(_, start)| *start).collect();
            if starts.len() < 2 {
                bounced |= starts.first().is_some_and(|&start| self.consolidate(track, start));
                continue;
            }
            let before = self.project.tracks[track].clips.clone();
            let clips = &mut self.project.tracks[track].clips;
            let mut parts: Vec<Clip> = Vec::new();
            clips.retain(|clip| {
                let picked = starts.contains(&clip.start);
                if picked {
                    parts.push(clip.clone());
                }
                !picked
            });
            if parts.len() < 2 {
                clips.extend(parts);
                clips.sort_by_key(|clip| clip.start);
                continue;
            }
            let start = parts[0].start;
            let end = parts.iter().map(Clip::end).max().unwrap_or(start);
            let bpm = self.project.bpm;
            match self.joined_file(&parts, start, end) {
                Ok(file) => {
                    self.project.tracks[track].place(joined(&parts, start, end, file), bpm);
                    self.selection.clips.retain(|(t, _)| *t != track);
                    self.selection.clips.push((track, start));
                }
                Err(reason) => self.refuse_join(track, before, reason),
            }
            along = true;
        }
        along || self.stack_selected_clips() | bounced
    }

    fn stack_selected_clips(&mut self) -> bool {
        let mut picked: Vec<(usize, Clip)> = self.picked_clips().into_iter().filter(|(track, _)| !self.project.tracks[*track].audio).collect();
        picked.sort_by_key(|(track, clip)| (*track, clip.start));
        picked.dedup_by_key(|(track, clip)| (*track, clip.start));
        let Some((top, first)) = picked.first().map(|(track, clip)| (*track, clip.start)).filter(|_| picked.len() > 1) else {
            return false;
        };
        let start = picked.iter().map(|(_, clip)| clip.start).min().unwrap_or(first);
        let end = picked.iter().map(|(_, clip)| clip.end()).max().unwrap_or(start);
        let bpm = self.project.bpm;
        let mut parts = self.project.tracks[top].take_between(start, end, bpm);
        parts.sort_by_key(|clip| clip.start != first);
        self.selection.clips.retain(|&(track, at)| !picked.iter().any(|(owner, clip)| (*owner, clip.start) == (track, at)));
        parts.extend(picked.into_iter().filter(|(track, _)| *track != top).map(|(_, clip)| clip));
        self.project.tracks[top].place(stacked(&parts, start, end), bpm);
        self.selection.clips.push((top, start));
        true
    }

    fn refuse_join(&mut self, track: usize, before: Vec<Clip>, reason: &'static str) {
        let name = before.iter().find_map(|clip| clip.file.as_ref()).map_or_else(|| String::from(AUDIO), |file| file.name.clone());
        self.project.tracks[track].clips = before;
        self.notice = Some(Notice { band: AUDIO, name, reason });
    }

    fn consolidate(&mut self, track: usize, start: i32) -> bool {
        let Some(clip) = self.project.tracks[track].clips.iter().find(|clip| clip.start == start).cloned() else {
            return false;
        };
        let Some(file) = clip.file.clone() else {
            return false;
        };
        let bpm = self.project.bpm;
        if self.pool.wave(&file.name).is_some_and(|wave| file.plays_whole(clip.length, wave.frames(), wave.rate, bpm)) {
            return false;
        }
        match self.bounced_file(std::slice::from_ref(&clip), clip.start, clip.end(), "cut") {
            Ok(cut) => {
                if let Some(held) = self.project.tracks[track].clips.iter_mut().find(|clip| clip.start == start) {
                    held.file = Some(cut);
                }
                true
            }
            Err(reason) => {
                self.notice = Some(Notice { band: AUDIO, name: file.name, reason });
                false
            }
        }
    }

    fn joined_file(&mut self, parts: &[Clip], start: i32, end: i32) -> Result<Option<File>, &'static str> {
        let Some(first) = parts.iter().find_map(|part| part.file.clone()) else {
            return Ok(None);
        };
        let bpm = self.project.bpm;
        let whole = parts.first().is_some_and(|part| part.start == start) && parts.last().is_some_and(|part| part.end() == end);
        if whole && parts.windows(2).all(|pair| continues(&pair[0], &pair[1], bpm)) {
            return Ok(Some(first));
        }
        self.bounced_file(parts, start, end, "join").map(Some)
    }

    pub(super) fn bounced(&mut self, parts: &[Clip], start: i32, end: i32, named: impl FnOnce(&str) -> String) -> Result<(Wave, f64), &'static str> {
        let files: Vec<&File> = parts.iter().filter_map(|part| part.file.as_ref()).collect();
        let Some(first) = files.first() else {
            return Err(UNREADABLE);
        };
        if !files.iter().all(|file| self.pool.wave(&file.name).is_some_and(|wave| wave.frames() > 0)) {
            return Err(UNREADABLE);
        }
        let name = self.pool.fresh_name(&named(&first.name));
        audio::bounce(name, parts, &self.pool.waves, self.project.bpm, start, end).ok_or(TOO_LONG)
    }

    fn bounced_file(&mut self, parts: &[Clip], start: i32, end: i32, suffix: &str) -> Result<File, &'static str> {
        let (wave, level) = self.bounced(parts, start, end, |first| format!("{first} {suffix}"))?;
        let name = wave.name.clone();
        self.pool.keep(wave, None);
        Ok(File { level, ..File::new(name) })
    }

    pub(super) fn picked_clips(&self) -> Vec<(usize, Clip)> {
        self.selection.clips.iter().filter_map(|&(track, start)| Some((track, self.project.tracks.get(track)?.clips.iter().find(|clip| clip.start == start)?.clone()))).collect()
    }

    pub(super) fn duplicate_selected_clips(&mut self) -> bool {
        let picked = self.picked_pieces();
        let Some(first) = picked.iter().map(|(_, piece)| piece.clip.start).min() else {
            return false;
        };
        let end = picked.iter().map(|(_, piece)| piece.clip.end()).max().unwrap_or(first);
        let span = end - first;
        if span > LONGEST_TICKS - end {
            return false;
        }
        let (bpm, carry) = (self.project.bpm, !self.project.lock);
        self.selection.clips.clear();
        for (track, piece) in picked {
            let copy = piece.shifted(span);
            self.selection.clips.push((track, copy.clip.start));
            self.project.tracks[track].lay(copy, carry, bpm);
        }
        true
    }

    pub(super) fn delete_selected_clips(&mut self) -> bool {
        if self.selection.clips.is_empty() {
            return false;
        }
        let carry = !self.project.lock;
        for (track, start) in std::mem::take(&mut self.selection.clips) {
            self.project.tracks[track].lift(start, carry);
        }
        true
    }

    pub(super) fn nudge_clips(&mut self, key: Key, shift: bool) -> bool {
        if self.picked_notes().is_some() {
            return false;
        }
        let picked = self.picked_clips();
        let (Some(first), Some(last)) = (picked.iter().map(|(_, clip)| clip.start).min(), picked.iter().map(|(_, clip)| clip.end()).max()) else {
            return false;
        };
        let step = if shift { TICKS_PER_BAR } else { self.timeline.grid_step() };
        let by = match key {
            Key::Left => grid::line_below(first - 1, step).max(0) - first,
            Key::Right => grid::line_after(grid::line_below(first, step), step) - first,
            _ => return false,
        };
        if by == 0 || by > LONGEST_TICKS - last {
            return false;
        }
        let phrase = self.lift_clips();
        self.lay_clips(phrase, by, 0);
        true
    }

    pub(super) fn drop_clip(&mut self, drag: ClipDrag) {
        if drag.resizing() {
            let clip = self.project.tracks[drag.track].clips.remove(drag.index);
            let resized = self.resized(drag, &clip);
            if let Some(key) = self.selection.clips.iter_mut().find(|key| **key == (drag.track, clip.start)) {
                key.1 = resized.start;
            }
            let bpm = self.project.bpm;
            self.project.tracks[drag.track].place(resized, bpm);
            return;
        }
        let Some((by, rows)) = self.clip_shift(drag) else {
            return;
        };
        let phrase = self.lift_clips();
        self.lay_clips(phrase, by, rows);
    }

    pub(super) fn picked_pieces(&self) -> Vec<(usize, Piece)> {
        self.picked_clips().into_iter().map(|(track, clip)| (track, self.project.tracks[track].piece(clip, true))).collect()
    }

    pub(super) fn lift_clips(&mut self) -> Vec<(usize, Piece)> {
        let carry = !self.project.lock;
        let mut phrase = Vec::new();
        for (track, start) in std::mem::take(&mut self.selection.clips) {
            if let Some(piece) = self.project.tracks.get_mut(track).and_then(|owner| owner.lift(start, carry)) {
                phrase.push((track, piece));
            }
        }
        phrase
    }

    pub(super) fn lay_clips(&mut self, phrase: Vec<(usize, Piece)>, by: i32, rows: i32) {
        let (bpm, carry) = (self.project.bpm, !self.project.lock);
        for (track, piece) in phrase {
            let track = (track as i32 + rows) as usize;
            let piece = piece.shifted(by);
            self.selection.clips.push((track, piece.clip.start));
            self.project.tracks[track].lay(piece, carry, bpm);
        }
    }

    pub(super) fn clip_shift(&self, drag: ClipDrag) -> Option<(i32, i32)> {
        let (track, lane) = self.lane_target(drag.at)?;
        let lane = if self.shift { lane.free() } else { lane };
        let anchor = &self.project.tracks[drag.track].clips[drag.index];
        let phrase = self.picked_clips();
        let first = phrase.iter().map(|(_, clip)| clip.start).min().unwrap_or(anchor.start);
        let last = phrase.iter().map(|(_, clip)| clip.end()).max().unwrap_or(anchor.end());
        let top = phrase.iter().map(|(track, _)| *track).min().unwrap_or(drag.track) as i32;
        let bottom = phrase.iter().map(|(track, _)| *track).max().unwrap_or(drag.track) as i32;
        let by = (lane.snapped(drag.at.0 - drag.grab) - anchor.start).max(-first).min(LONGEST_TICKS - last);
        let rows = (track as i32 - drag.track as i32).clamp(-top, self.project.tracks.len() as i32 - 1 - bottom);
        let pinned = phrase.iter().any(|(track, _)| {
            let target = &self.project.tracks[(*track as i32 + rows) as usize];
            self.hidden(*track) || target.group || target.audio != self.project.tracks[*track].audio
        });
        Some((by, if pinned { 0 } else { rows }))
    }

    pub(super) fn chord_clip(&self, drag: ChordDrag, start: i32) -> Clip {
        let roman = self.project.chord(drag.chord).roman;
        Clip::chord(start, roman, &self.project.chord_notes(drag.chord))
    }

    pub(super) fn resized(&self, drag: ClipDrag, clip: &Clip) -> Clip {
        let lane = self.lane(self.track_tile(self.track_slot(drag.track)));
        let lane = if self.shift { lane.free() } else { lane };
        let (tick, step) = (lane.snapped(drag.at.0), lane.snap);
        let mut resized = clip.clone();
        match drag.hit {
            ClipHit::Start => {
                let start = if tick > clip.start { tick.min(grid::line_below(clip.end() - step, step)).max(clip.start) } else { tick.max(0) };
                resized.trim_head(start - clip.start, self.project.bpm);
            }
            ClipHit::Body | ClipHit::End => {
                let end = if tick < clip.end() { tick.max(grid::line_above(clip.start + step, step)).min(clip.end()) } else { tick.min(LONGEST_TICKS) };
                resized.resize(end - clip.start);
            }
        }
        resized
    }

    pub(super) fn sweep_from(&mut self, x: i32, y: i32, hit: Option<(usize, usize)>) {
        self.drag = Some(Drag::Marquee(Marquee { origin: (x, y), at: (x, y), armed: false, hit, before: self.selection.clips.clone() }));
    }

    pub(super) fn swept_into(&mut self, marquee: &Marquee) {
        let mut picked = marquee.before.clone();
        for key in self.swept_clips(grid::marquee(marquee.origin, marquee.at)) {
            if !picked.contains(&key) {
                picked.push(key);
            }
        }
        self.selection.clips = picked;
        self.selection.paste_point = None;
    }

    pub(super) fn lanes_bounds(&self) -> Rect {
        self.lanes_region().intersect(self.track_viewport())
    }

    pub(super) fn swept_clips(&self, sweep: Rect) -> Vec<(usize, i32)> {
        let sweep = sweep.intersect(self.lanes_bounds());
        self.track_slots()
            .into_iter()
            .flat_map(|(slot, track)| {
                let lane = self.lane(self.track_tile(slot));
                let owners = if self.project.tracks[track].folded { self.project.members_of(track) } else { vec![track] };
                owners.into_iter().flat_map(move |owner| lane.swept_clips(&self.project.tracks[owner].clips, sweep).map(move |clip| (owner, clip.start)))
            })
            .collect()
    }

    pub(super) fn landing(&self, track: usize) -> Option<usize> {
        let owner = self.project.tracks.get(track)?;
        if owner.group { self.project.members_of(track).first().copied() } else { Some(track) }
    }

    pub(super) fn lane_target(&self, at: (i32, i32)) -> Option<(usize, Lane)> {
        let viewport = self.track_viewport();
        self.track_slots().into_iter().find_map(|(slot, track)| {
            let lane = self.lane(self.track_tile(slot));
            if !lane.rect.intersect(viewport).contains(at.0, at.1) {
                return None;
            }
            self.landing(track).map(|target| (target, lane))
        })
    }

    pub(super) fn hold_cut(&mut self, seconds: f32) -> bool {
        let mut changed = false;
        if let Some((beat, left)) = self.cut_flash {
            self.cut_flash = (left > seconds).then_some((beat, left - seconds));
            changed = true;
        }
        let Some(mut drag) = self.clip_drag().filter(|drag| !drag.armed && !drag.resizing()) else {
            return changed;
        };
        let tick = self.cut_tick(drag);
        if !self.cut_targets(tick) {
            return changed;
        }
        drag.held += seconds;
        if drag.held < CUT_HOLD_SECONDS {
            self.drag = Some(Drag::Clip(drag));
            return true;
        }
        self.drag = None;
        self.cut_picked(tick);
        self.cut_flash = Some((tick, CUT_FLASH_SECONDS));
        self.finish_edit();
        true
    }

    pub(super) fn cut_tick(&self, drag: ClipDrag) -> i32 {
        self.lane(self.track_tile(self.track_slot(drag.track))).snapped(drag.origin.0)
    }

    pub(super) fn cut_targets(&self, tick: i32) -> bool {
        self.selection.clips.iter().any(|&(track, start)| self.project.tracks.get(track).and_then(|owner| owner.clips.iter().find(|clip| clip.start == start)).is_some_and(|clip| clip.start < tick && tick < clip.end()))
    }

    pub(super) fn cut_picked(&mut self, tick: i32) {
        let shown = self.selection.clips.last().copied();
        let bpm = self.project.bpm;
        for (track, start) in self.selection.clips.clone() {
            if self.project.tracks[track].split(start, tick, bpm) {
                self.selection.clips.push((track, tick));
            }
        }
        if let Some(shown) = shown {
            self.selection.clips.retain(|picked| *picked != shown);
            self.selection.clips.push(shown);
        }
    }

    pub(super) fn track_lane_down(&mut self, x: i32, y: i32) -> bool {
        self.roll.unpick();
        if self.shift {
            self.sweep_from(x, y, None);
            return false;
        }
        let (was, had) = (self.selection.paste_point, !self.selection.clips.is_empty() || self.selection.region.is_some());
        self.unpick_elsewhere();
        self.selection.clips.clear();
        self.selection.paste_point = self.lane_target((x, y)).map(|(track, lane)| (track, lane.floored(x).max(0)));
        self.sweep_region(x, y);
        had || self.selection.paste_point != was
    }

    pub(super) fn clip_down(&mut self, track: usize, index: usize, hit: ClipHit, x: i32, y: i32) -> bool {
        self.roll.unpick();
        self.unpick_elsewhere();
        self.view.palette_shown = false;
        if self.project.tracks[track].audio {
            self.set_aside(Aside::Wave);
        }
        if self.shift {
            self.sweep_from(x, y, Some((track, index)));
            return false;
        }
        let lane = self.lane(self.track_tile(self.track_slot(track)));
        let start = self.project.tracks[track].clips[index].start;
        if !self.selection.clips.contains(&(track, start)) {
            self.selection.clips = vec![(track, start)];
        }
        self.drag = Some(Drag::Clip(ClipDrag { track, index, hit, grab: x - lane.x_of(start), origin: (x, y), at: (x, y), armed: false, held: 0.0 }));
        true
    }
}

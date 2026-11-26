use super::*;

impl Renderer {
    pub(super) fn begin_rehearsal(&mut self, rehearsal: Option<Box<Rehearsal>>) {
        self.end_rehearsal();
        self.rehearsal = rehearsal;
        self.rehearsal_cursor = 0;
        self.rehearsal_began = self.frame;
    }

    pub(super) fn end_rehearsal(&mut self) {
        let now = self.frame;
        for active in self.rehearsed.drain(..) {
            each_midi(&self.table, self.count, active.track, now, |runner, stream| runner.note_off(active.pitch, stream));
        }
        if let Some(rehearsal) = self.rehearsal.take() {
            self.retire(Displaced::Rehearsal(rehearsal));
        }
    }

    pub(super) fn retrack_rehearsal(&mut self, landed: impl Fn(usize) -> Option<usize>) {
        let mut retrack = |track: &mut usize| match landed(*track) {
            Some(to) => {
                *track = to;
                true
            }
            None => false,
        };
        if let Some(rehearsal) = &mut self.rehearsal {
            rehearsal.tracks.retain_mut(&mut retrack);
        }
        self.rehearsed.retain_mut(|active| retrack(&mut active.track));
    }

    pub(super) fn retime_rehearsal(&mut self, now: i64, ratio: f64) {
        self.rehearsal_began = now - ((now - self.rehearsal_began) as f64 * ratio).round() as i64;
    }

    pub(super) fn rehearse(&mut self, frames: usize) {
        let Some(rehearsal) = self.rehearsal.take() else {
            return;
        };
        let (now, until, frames_per_beat) = (self.frame, self.frame + frames as i64, self.frames_per_beat);
        loop {
            let began = self.rehearsal_began;
            let frame_of = |beat: f64| (began + (beat * frames_per_beat).round() as i64).max(now);
            let inside = |beat: f64| frame_of(beat) < until;
            let mut index = 0;
            while index < self.rehearsed.len() {
                if inside(self.rehearsed[index].end) {
                    let active = self.rehearsed.swap_remove(index);
                    let off = frame_of(active.end);
                    each_midi(&self.table, self.count, active.track, now, |runner, stream| runner.cue_off(active.pitch, off, stream));
                } else {
                    index += 1;
                }
            }
            while let Some(note) = rehearsal.notes.get(self.rehearsal_cursor).filter(|note| note.start < rehearsal.beats && inside(note.start)) {
                self.rehearsal_cursor += 1;
                let (on, end) = (frame_of(note.start), note.end.min(rehearsal.beats));
                let held = !inside(end);
                for &track in &rehearsal.tracks {
                    if held && self.rehearsed.len() == self.rehearsed.capacity() {
                        continue;
                    }
                    self.restart_lanes(track);
                    each_struck(&self.table, self.count, track, now, note.velocity, |runner, stream, velocity| runner.cue_on(note.pitch, velocity, note.articulation, on, stream));
                    if held {
                        self.rehearsed.push(Active { track, pitch: note.pitch, end });
                    } else {
                        let off = frame_of(end);
                        each_midi(&self.table, self.count, track, now, |runner, stream| runner.cue_off(note.pitch, off, stream));
                    }
                }
            }
            if !inside(rehearsal.beats) {
                break;
            }
            self.rehearsal_began = frame_of(rehearsal.beats).max(began + 1);
            self.rehearsal_cursor = 0;
        }
        self.rehearsal = Some(rehearsal);
    }
}

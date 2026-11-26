use super::*;
use crate::project::{LEAST_BPM, MOST_BPM};

pub(super) const TEMPO_FINE_STEPS_PER_BPM: f64 = 100.0;

#[derive(Clone, PartialEq)]
pub(super) struct Latch {
    pub(super) chord: Chord,
    pub(super) tracks: Vec<usize>,
}

#[derive(Clone, Copy)]
pub(super) struct TempoDrag {
    pub(super) origin_y: i32,
    pub(super) origin: f64,
    pub(super) y: i32,
}

impl App {
    pub(super) fn scale_tempo(&mut self, ratio: f64) -> bool {
        let bpm = ((self.project.bpm * ratio * TEMPO_FINE_STEPS_PER_BPM).round() / TEMPO_FINE_STEPS_PER_BPM).clamp(LEAST_BPM, MOST_BPM);
        let changed = bpm != self.project.bpm;
        self.project.bpm = bpm;
        changed
    }

    pub fn audio_started(&mut self, sample_rate: f64, max_frames: usize, frames: u32) -> Renderer {
        let (audio, renderer) = Audio::open(sample_rate, max_frames, Seer::spawn());
        self.settings.buffer = frames;
        self.audio = Some(audio);
        self.install_audio();
        renderer
    }

    pub fn audio_stopped(&mut self) {
        let taken = self.live.take.is_some();
        self.print_take();
        self.audio = None;
        self.live.sounding.clear();
        self.roll_note = None;
        self.stop();
        if taken {
            self.finish_edit();
        }
    }

    pub fn tick(&mut self, seconds: f32) -> bool {
        self.gesture = self.gesture.map(|(step, rest)| (step, rest - seconds)).filter(|(_, rest)| *rest > 0.0);
        let served = self.serve_calls();
        self.tick_listen();
        let exporting = self.tick_export(seconds) | self.tick_ducks();
        let (insert, output) = (self.watched_insert(), self.heard());
        let meters = self.audio.as_mut().is_some_and(|audio| {
            audio.aim(Scope::Insert, insert);
            audio.aim(Scope::Output, output);
            audio.tick(seconds)
        }) | self.tick_deepest(seconds);
        let playing = self.playing;
        if self.audio.as_mut().is_some_and(|audio| audio.settled(playing)) {
            self.sync_audio();
        }
        if self.playing {
            self.playhead = self.audio.as_ref().map_or(self.playhead, |audio| audio.position().beat);
            if self.settings.follow {
                self.follow_playhead();
            }
        }
        let panned = self.throttle_pan(f64::from(seconds)) | self.throttle_wave_pan(f64::from(seconds)) | self.hold_cut(seconds) | self.disk.linger_saved(seconds) | self.linger_copied(seconds) | (self.roll_shown() && self.roll.tick(seconds)) | self.pool.tick_audition(seconds);
        if self.small() {
            self.bounce(seconds);
            return true;
        }
        meters || self.playing || panned || self.pool.audition.is_some() || self.lfo_shown() || exporting || served
    }

    fn lfo_shown(&self) -> bool {
        if self.audio.is_none() {
            return false;
        }
        let owners = self.project.tracks.iter().map(|track| &track.automation).chain(self.project.returns().map(|ret| &ret.automation));
        let gain_waved = owners.into_iter().any(|lanes| lanes.lfos.iter().any(|lfo| lfo.aim.row().is_none() && lfo.amount > 0.0) || lanes.curves.iter().any(|curve| curve.aim.row().is_none() && curve.lfo.is_some()));
        let face_waved = self.open.is_some_and(|row| {
            let lanes = self.project.automation(self.host());
            lanes.lfos.iter().any(|lfo| lfo.aim.row_or_top() == row && lfo.amount > 0.0) || lanes.curves.iter().any(|curve| curve.aim.row_or_top() == row && curve.lfo.is_some())
        });
        gain_waved || face_waved
    }

    pub(super) fn clock(&self) -> String {
        let seconds = (self.playhead * 60.0 / self.project.bpm.max(1.0)) as i64;
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }

    pub(super) fn follow_playhead(&mut self) {
        if self.width == 0 {
            return;
        }
        let lane = self.lane(self.track_tile(0));
        let x = lane.x_of_beat(self.playhead);
        if x < lane.rect.x || x >= self.width {
            self.timeline.grid_scroll = (self.playhead * self.timeline.beat_w) as i32;
        }
    }

    pub(super) fn rewind(&mut self) {
        self.mark_start(0);
        self.timeline.grid_scroll = 0;
    }

    pub(super) fn step_playhead(&mut self, key: Key, shift: bool) -> bool {
        if self.roll_shown() {
            return false;
        }
        let step = if shift { self.timeline.grid_step() } else { TICKS_PER_BAR };
        let from = if self.playing { ticks(self.playhead) } else { self.play_from };
        let tick = match key {
            Key::Right => grid::line_after(grid::line_below(from, step), step),
            Key::Left => grid::line_below(from - 1, step).max(0),
            _ => return false,
        };
        self.place_playhead(tick);
        self.follow_playhead();
        true
    }

    pub(super) fn play(&mut self) {
        let from = beats(self.play_from);
        let range = self.loop_beats();
        if let (Some(take), Some(at)) = (self.live.take.as_mut(), self.audio.as_ref().map(Audio::position)) {
            take.seek(at, range, from);
        }
        self.playing = true;
        self.playhead = from;
        if let Some(audio) = &mut self.audio {
            audio.play(from);
        }
    }

    pub(super) fn record(&mut self) {
        if self.live.take.is_some() {
            self.print_take();
            return;
        }
        if !self.playing {
            self.play();
        }
        if !self.project.tracks[self.selection.track].generates() {
            return;
        }
        let mut held: Vec<u8> = self.live.keys.iter().map(|note| note.pitch).chain(self.live.sounding.iter().map(|(_, note)| *note)).collect();
        held.sort_unstable();
        held.dedup();
        self.live.take = self.audio.as_ref().map(|audio| Take::new(audio.position(), held));
    }

    pub(super) fn take_preview(&self, track: usize) -> Option<Vec<Clip>> {
        let take = self.live.take.as_ref().filter(|_| track == self.selection.track)?;
        let audio = self.audio.as_ref()?;
        Some(record::lay(&self.project.tracks[track].clips, &take.preview(audio.position(), self.loop_beats()), self.timeline.snap.beats()))
    }

    pub(super) fn under_playhead(&self) -> Option<Sounding> {
        if !self.playing || self.monitored.is_some() {
            return None;
        }
        self.project.tracks[self.selection.track].sounding(self.playhead)
    }

    pub(super) fn print_take(&mut self) {
        let (Some(take), Some(audio)) = (self.live.take.take(), self.audio.as_ref()) else {
            return;
        };
        let notes = take.close(audio.position(), self.loop_beats());
        if notes.is_empty() {
            return;
        }
        let grid = self.timeline.snap.beats();
        let track = &mut self.project.tracks[self.selection.track];
        track.clips = record::lay(&track.clips, &notes, grid);
    }

    pub(super) fn loop_beats(&self) -> Option<(f64, f64)> {
        self.wave_loop_beats().or_else(|| self.ruler.range().map(Loop::beats))
    }

    pub(super) fn stop(&mut self) {
        self.playing = false;
        self.live.latched = None;
        self.stop_audition();
        self.release_keys(|_| true);
        self.print_take();
        self.roll.silence();
        self.playhead = beats(self.play_from);
        if let Some(audio) = &self.audio {
            audio.stop();
            audio.place(self.playhead);
        }
    }

    pub(super) fn halt(&mut self) {
        self.play_from = 0;
        self.ruler.remember(None);
        self.stop();
    }

    pub(super) fn mark_start(&mut self, tick: i32) {
        self.play_from = tick;
        if !self.playing {
            self.playhead = beats(tick);
            if let Some(audio) = &self.audio {
                audio.place(self.playhead);
            }
        }
    }

    pub(super) fn mark_loop(&mut self) {
        if let Some(range) = self.ruler.range() {
            self.mark_start(range.start);
        }
    }

    pub(super) fn place_playhead(&mut self, tick: i32) {
        self.mark_start(tick);
        if self.playing {
            self.play();
        }
    }

    pub(super) fn transport_fields(&self, fields: &mut Fields) {
        fields.number("playheadBeat", beats(self.play_from), 0.0);
        if let Some(range) = self.ruler.range() {
            fields.push("loopStartBeat", Json::Number(beats(range.start)));
            fields.push("loopEndBeat", Json::Number(beats(range.end)));
        }
    }

    pub(super) fn transport_manual(fields: &mut Fields) {
        fields.push("playheadBeat", Json::from("number 0 or more, default 0: where play starts from"));
        fields.push("loopStartBeat", Json::from("number 0 or more: the loop's start; with loopEndBeat, play cycles between them and an export covers the loop"));
        fields.push("loopEndBeat", Json::from("number above loopStartBeat: the loop's end; both left out for no loop"));
    }

    pub(super) fn transport_from_json(&mut self, json: &Json) {
        let tick = |key: &str, default: f64| ticks(number(Some(json), key, default)).min(LONGEST_TICKS);
        self.ruler.remember(Some(Loop { start: tick("loopStartBeat", -1.0), end: tick("loopEndBeat", -1.0) }));
        self.place_playhead(tick("playheadBeat", 0.0).max(0));
    }

    pub(super) fn install_audio(&mut self) {
        self.live.install();
        self.roll_note = None;
        let range = self.loop_beats();
        let solos = self.solos();
        let preview = self.preview_gain();
        if let Some(audio) = &mut self.audio {
            audio.install(&self.project, self.soloed, &solos, &self.pool.waves);
            audio.set_loop(range);
            audio.place(self.playhead);
            audio.preview_gain(preview);
        }
    }

    pub(super) fn chord_tracks(&self) -> Vec<usize> {
        let picked = self.selection.picked_tracks();
        if self.live.sounding.is_empty() || picked.iter().any(|&track| self.project.tracks[track].generates()) {
            return picked;
        }
        let mut sounding: Vec<usize> = self.live.sounding.iter().map(|(track, _)| *track).collect();
        sounding.sort_unstable();
        sounding.dedup();
        sounding
    }

    pub(super) fn sync_audio(&mut self) {
        for (track, generator) in self.generators.iter_mut().enumerate() {
            if let Some(solo) = generator.core
                && !self.project.tracks.get(track).is_some_and(|owner| owner.rack.get(solo.row).is_some_and(Row::is_core))
            {
                generator.core = None;
            }
        }
        let solos = self.solos();
        let range = self.loop_beats();
        if let Some(audio) = &mut self.audio {
            audio.reclaim();
            audio.sync(&self.project, self.soloed, &solos, &self.pool.waves);
            audio.set_loop(range);
        }
        self.sync_notes();
    }

    pub(super) fn sync_notes(&mut self) {
        let roll_note = self.edited_clip().map(|(track, _)| track).zip(self.roll.sounding());
        let range = self.loop_beats();
        let held = self.view.palette_shown.then(|| self.palettes.iter().find_map(|palette| palette.playing(&self.project))).flatten().map(|chord| Latch { chord, tracks: self.chord_tracks() }).or_else(|| self.live.latched.clone());
        let Some(audio) = &mut self.audio else {
            return;
        };
        let at = audio.position();
        let notes: Vec<u8> = held.as_ref().map(|latch| self.project.chord_notes(latch.chord)).unwrap_or_default();
        let tracks = self.project.leading(held.map(|latch| latch.tracks).unwrap_or_default());
        let wanted: Vec<(usize, u8)> = tracks.iter().flat_map(|track| notes.iter().map(|note| (*track, *note))).collect();
        for (track, note) in self.live.sounding.iter().filter(|sounding| !wanted.contains(sounding)) {
            audio.note_off(*track, *note);
        }
        for (track, note) in wanted.iter().filter(|wanted| !self.live.sounding.contains(wanted)) {
            audio.note_on(*track, *note, VELOCITY);
        }
        if let Some(take) = &mut self.live.take {
            let mut held: Vec<u8> = self.live.sounding.iter().map(|(_, note)| *note).collect();
            held.sort_unstable();
            held.dedup();
            for note in held.iter().filter(|note| !notes.contains(note)) {
                take.note_off(at, range, *note);
            }
            for note in notes.iter().filter(|note| !held.contains(note)) {
                take.note_on(at, range, *note);
            }
        }
        self.live.sounding = wanted;
        if roll_note != self.roll_note {
            if let Some((track, pitch)) = self.roll_note {
                for track in self.project.leading(vec![track]) {
                    audio.note_off(track, pitch);
                }
            }
            if let Some((track, pitch)) = roll_note {
                for track in self.project.leading(vec![track]) {
                    audio.note_on(track, pitch, VELOCITY);
                }
            }
            self.roll_note = roll_note;
        }
        if std::mem::take(&mut self.live.restrike) {
            for (track, note) in self.live.sounding.iter().copied().chain(self.live.keys.iter().map(|key| (key.track, key.pitch))) {
                audio.note_off(track, note);
                audio.note_on(track, note, VELOCITY);
            }
        }
    }

    pub(super) fn ruler_down(&mut self, x: i32, y: i32, clicks: i32) -> bool {
        let lane = self.ruler_lane();
        let down = self.ruler.down(lane, x, y, clicks);
        match down {
            Some(ruler::Down::Bar) => self.mark_loop(),
            Some(ruler::Down::Seek(beat)) => self.place_playhead(beat),
            None => {}
        }
        down.is_some()
    }

    pub(super) fn tempo_down(&mut self, y: i32, reset: bool) -> bool {
        if reset {
            self.project.bpm = DEFAULT_BPM;
        } else {
            self.drag = Some(Drag::Tempo(TempoDrag { origin_y: y, origin: self.project.bpm, y }));
        }
        true
    }

    pub(super) fn ruler_hover(&mut self, x: i32) -> bool {
        let hit = (self.hover == Hover::Ruler).then(|| self.ruler.hit(self.ruler_lane(), x));
        self.ruler.hover(hit)
    }

    pub(super) fn bar_hover(&mut self, x: i32) -> bool {
        let timeline = matches!(self.hover, Hover::Ruler | Hover::Tile(_) | Hover::Clip(..) | Hover::Lane(_) | Hover::Append(_));
        let bar = timeline.then(|| self.ruler_lane().bar_at(x) / TICKS_PER_BAR + 1);
        let changed = bar != self.bar_under;
        self.bar_under = bar;
        changed
    }
}

use super::*;
use sdk::param::{Scale, linear};
use crate::project::PITCH_BANDS;

const TUNE: &str = "TUNE";
const TUNE_MIN: i32 = -48;
const TUNE_MAX: i32 = 48;
const TUNE_SCALE: Scale = linear(TUNE_MIN as f64, TUNE_MAX as f64);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Source {
    row: usize,
    osc: usize,
}

#[derive(Clone, Copy)]
pub(super) struct GhostDrag {
    track: usize,
    source: Source,
    knob: i32,
    tick: i32,
    origin: (i32, i32),
    tune: i32,
    armed: bool,
}

fn oscillators(layer: &Layer) -> [(f64, f64); PITCH_BANDS.len()] {
    layer.patch.oscs().map(|osc| (osc.level, osc.tune))
}

fn semitones(travel: f64) -> i32 {
    TUNE_SCALE.value(travel).round() as i32
}

fn travel(semitones: i32) -> f32 {
    TUNE_SCALE.travel(f64::from(semitones)) as f32
}

fn start_tick(clip: &Clip, note: &Note) -> i32 {
    clip.start + ticks(f64::from(note.start))
}

impl App {
    pub(super) fn ghost_sources(&self, track: usize) -> Vec<(Source, i32)> {
        let owner = &self.project.tracks[track];
        if !self.settings.ghost_notes || !owner.generates() {
            return Vec::new();
        }
        let mut sources = Vec::new();
        for (row, slot) in owner.rack.iter().enumerate() {
            let Row::Core(layer) = slot else {
                continue;
            };
            if layer.level <= 0.0 {
                continue;
            }
            for (osc, (level, tune)) in oscillators(layer).into_iter().enumerate() {
                if level > 0.0 {
                    sources.push((Source { row, osc }, tune as i32));
                }
            }
        }
        sources
    }

    fn tune_at(&self, track: usize, source: Source, knob: i32, tick: i32) -> i32 {
        match self.project.tracks[track].automation.curve(&Aim::knob(source.row, PITCH_BANDS[source.osc], TUNE)) {
            Some(curve) => semitones(curve.at(beats(tick))),
            None => knob,
        }
    }

    pub(super) fn ghosts_of(&self, track: usize, clip: &Clip) -> Vec<roll::Ghost> {
        let mut ghosts = Vec::new();
        for (index, (source, knob)) in self.ghost_sources(track).into_iter().enumerate() {
            for note in &clip.notes {
                let tick = start_tick(clip, note);
                let tune = self.tune_at(track, source, knob, tick);
                if tune != 0
                    && let Ok(pitch) = u8::try_from(i32::from(note.pitch) + tune)
                    && pitch <= 127
                {
                    ghosts.push(roll::Ghost { note: Note { pitch, ..*note }, source: index, tick });
                }
            }
        }
        ghosts
    }

    pub(super) fn held_ghosts(&self, track: usize) -> Option<(usize, i32)> {
        let Some(Drag::Ghost(drag)) = &self.drag else {
            return None;
        };
        if drag.track != track {
            return None;
        }
        let source = self.ghost_sources(track).iter().position(|(source, _)| *source == drag.source)?;
        Some((source, drag.tick))
    }

    pub(super) fn ghost_down(&mut self, track: usize, ghost: roll::Ghost, x: i32, y: i32) -> bool {
        let Some(&(source, knob)) = self.ghost_sources(track).get(ghost.source) else {
            return false;
        };
        let tune = self.tune_at(track, source, knob, ghost.tick);
        self.drag = Some(Drag::Ghost(GhostDrag { track, source, knob, tick: ghost.tick, origin: (x, y), tune, armed: false }));
        true
    }

    pub(super) fn ghost_moved(&mut self, drag: GhostDrag, x: i32, y: i32) -> bool {
        let (_, ry) = self.roll_point(x, y);
        let rows = self.roll.rows_from(drag.origin.1, ry);
        self.drag = Some(Drag::Ghost(GhostDrag { armed: drag.armed || rows != 0, ..drag }));
        self.hold_tune(drag, (drag.tune + rows).clamp(TUNE_MIN, TUNE_MAX))
    }

    fn next_hold(&self, track: usize, tick: i32) -> Option<i32> {
        let owner = &self.project.tracks[track];
        let starts = owner.clips.iter().flat_map(|clip| clip.notes.iter().filter(|note| note.start < clip.beats()).map(move |note| start_tick(clip, note)));
        starts.filter(|start| *start > tick).min()
    }

    fn hold_tune(&mut self, drag: GhostDrag, tune: i32) -> bool {
        if self.project.lock {
            return false;
        }
        let next = self.next_hold(drag.track, drag.tick);
        let lanes = &mut self.project.tracks[drag.track].automation;
        let was = lanes.clone();
        lanes.hold(&Aim::knob(drag.source.row, PITCH_BANDS[drag.source.osc], TUNE), drag.tick, travel(tune), next, travel(drag.knob));
        *lanes != was
    }

    pub(super) fn ghost_released(&mut self, drag: GhostDrag) {
        let Some((track, index)) = self.edited_clip().filter(|_| !drag.armed) else {
            return;
        };
        self.roll.tap(drag.origin.0, drag.origin.1, &mut self.project.tracks[track].clips[index]);
    }
}

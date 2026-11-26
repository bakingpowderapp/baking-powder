use crate::arp::Articulation;
use crate::audio::Position;
use crate::flatten;
use crate::project::{BEATS_PER_BAR, Clip, SongNote, TICKS_PER_BAR, VELOCITY, beats};

#[derive(Clone)]
struct Open {
    pitch: u8,
    start: f64,
}

#[derive(Clone)]
pub struct Take {
    open: Vec<Open>,
    notes: Vec<SongNote>,
    wraps: u16,
}

fn end(notes: &mut Vec<SongNote>, open: &Open, at: f64) {
    if at > open.start {
        notes.push(SongNote { start: open.start, end: at, pitch: open.pitch, velocity: VELOCITY, articulation: Articulation::PLAIN });
    }
}

impl Take {
    pub fn new(at: Position, held: impl IntoIterator<Item = u8>) -> Take {
        Take { open: held.into_iter().map(|pitch| Open { pitch, start: at.beat }).collect(), notes: Vec::new(), wraps: at.wraps }
    }

    fn reach(&mut self, at: Position, range: Option<(f64, f64)>) {
        let wraps = at.wraps.wrapping_sub(self.wraps);
        self.wraps = at.wraps;
        let Some((start, stop)) = range.filter(|_| wraps > 0) else {
            return;
        };
        for open in &mut self.open {
            end(&mut self.notes, open, stop);
            if wraps > 1 {
                end(&mut self.notes, &Open { pitch: open.pitch, start }, stop);
            }
            open.start = start;
        }
    }

    pub fn note_on(&mut self, at: Position, range: Option<(f64, f64)>, pitch: u8) {
        self.note_off(at, range, pitch);
        self.open.push(Open { pitch, start: at.beat });
    }

    pub fn note_off(&mut self, at: Position, range: Option<(f64, f64)>, pitch: u8) {
        self.reach(at, range);
        if let Some(index) = self.open.iter().position(|open| open.pitch == pitch) {
            let open = self.open.swap_remove(index);
            end(&mut self.notes, &open, at.beat);
        }
    }

    pub fn seek(&mut self, at: Position, range: Option<(f64, f64)>, from: f64) {
        self.reach(at, range);
        for open in &mut self.open {
            end(&mut self.notes, open, at.beat);
            open.start = from;
        }
    }

    pub fn close(mut self, at: Position, range: Option<(f64, f64)>) -> Vec<SongNote> {
        self.reach(at, range);
        for open in &self.open {
            end(&mut self.notes, open, at.beat);
        }
        self.notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
        self.notes
    }

    pub fn preview(&self, at: Position, range: Option<(f64, f64)>) -> Vec<SongNote> {
        self.clone().close(at, range)
    }
}

fn snapped(beat: f64, grid: f64) -> f64 {
    (beat / grid).round() * grid
}

fn bar_below(beat: f64) -> i32 {
    (beat / f64::from(BEATS_PER_BAR)).floor() as i32 * TICKS_PER_BAR
}

fn bar_above(beat: f64) -> i32 {
    (beat / f64::from(BEATS_PER_BAR)).ceil() as i32 * TICKS_PER_BAR
}

pub fn lay(clips: &[Clip], take: &[SongNote], grid: f64) -> Vec<Clip> {
    let notes: Vec<SongNote> = take
        .iter()
        .map(|note| {
            let start = snapped(note.start, grid);
            SongNote { start, end: snapped(note.end, grid).max(start + grid), ..*note }
        })
        .collect();
    let (Some(first), Some(last)) = (notes.iter().map(|note| note.start).min_by(f64::total_cmp), notes.iter().map(|note| note.end).max_by(f64::total_cmp)) else {
        return clips.to_vec();
    };
    let (mut cursor, close) = (bar_below(first), bar_above(last));
    let starts_between = |from: i32, to: i32| notes.iter().any(|note| note.start >= beats(from) && note.start < beats(to));
    let mut laid = clips.to_vec();
    for (start, stop) in clips.iter().map(|clip| (clip.start, clip.end())).chain(std::iter::once((close, close))) {
        let edge = start.min(close);
        if edge > cursor && starts_between(cursor, edge) {
            laid.push(Clip { start: cursor, length: edge - cursor, label: String::new(), notes: Vec::new(), file: None });
        }
        cursor = cursor.max(stop);
        if cursor >= close {
            break;
        }
    }
    laid.sort_by_key(|clip| clip.start);
    flatten::overdub(&laid, &notes)
}

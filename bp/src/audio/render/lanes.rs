use super::*;
use coresynth::BEND_FRAMES;
use crate::project::{Sweep, struck};

const BRINK: f64 = 1e-9;

fn swept(curve: &Curve, at: f64, segment: Segment, frames: usize) -> (f32, f32) {
    let brink = if segment.playing { at + frames as f64 / segment.frames_per_beat - BRINK } else { at };
    (position_to_gain(curve.at(at) as f32), position_to_gain(curve.at(brink) as f32))
}

const RESTARTS: usize = 64;

#[derive(Clone, Copy, PartialEq)]
enum Source {
    Input,
    Row(usize),
}

fn source(rows: &Rows, aim: &Aim) -> Option<Source> {
    let (target, mut chain, mut last) = (aim.row(), None, None);
    for (position, slot) in rows.iter().enumerate() {
        let Some(row) = grab(slot) else {
            continue;
        };
        match row.as_ref() {
            None => break,
            Some(Insert::Midi { .. }) if target == Some(position) => return Some(chain.map_or(Source::Input, Source::Row)),
            Some(Insert::Midi { .. }) => (chain, last) = (Some(position), Some(position)),
            Some(Insert::Bus { master, .. }) => chain = chain.and(Some(*master)),
            Some(Insert::Core { .. } | Insert::Effect { .. }) if target == Some(position) => return chain.map(Source::Row),
            Some(Insert::Core { .. } | Insert::Effect { .. }) => {}
        }
    }
    if target.is_none() { last.map(Source::Row) } else { None }
}

struct Restarts {
    at: [f64; RESTARTS],
    len: usize,
}

impl Restarts {
    const NONE: Restarts = Restarts { at: [0.0; RESTARTS], len: 0 };

    fn of(landings: impl Iterator<Item = f64>) -> Restarts {
        let mut restarts = Restarts::NONE;
        for landing in landings.take(RESTARTS) {
            restarts.at[restarts.len] = landing;
            restarts.len += 1;
        }
        restarts
    }

    fn landings(&self) -> &[f64] {
        &self.at[..self.len]
    }
}

#[derive(Clone, Copy)]
pub(super) struct Clocking {
    until: Option<i64>,
    rested: f64,
    rest_seconds: f64,
}

impl Renderer {
    pub(super) fn clocking(&self) -> Clocking {
        Clocking { until: None, rested: self.rested, rest_seconds: self.rest_seconds }
    }

    pub(super) fn clocking_of(&self, track: usize) -> Clocking {
        match self.window(track) {
            Some(window) if window.moved => Clocking { until: Some(window.to), rested: self.line().beat_at(window.to), rest_seconds: (self.frame - window.to).max(0) as f64 / self.sample_rate },
            _ => self.clocking(),
        }
    }

    fn clocked(&self, track: usize, at: f64, lane: Lane, frames: usize) -> Option<(f64, Lane, Clocking)> {
        if self.window(track).is_some_and(|window| window.waits(self.frame, frames)) {
            return None;
        }
        let clocking = self.clocking_of(track);
        let Some(until) = clocking.until else {
            return Some((at, lane, clocking));
        };
        if self.frame < until {
            let beat = self.line().beat_at(self.frame);
            return Some((beat, Lane { beats: beat, seconds: beat * 60.0 / self.bpm }, clocking));
        }
        let resting = (self.frame - until) as f64;
        Some((clocking.rested, Lane { beats: clocking.rested + resting / self.frames_per_beat, seconds: clocking.rested * 60.0 / self.bpm + clocking.rest_seconds }, clocking))
    }

    pub(super) fn segment_of(&self, back: usize, clocking: Clocking) -> Segment {
        let frame = self.frame - back as i64;
        let segment = match clocking.until {
            Some(until) if frame >= until => Segment::stopped(until),
            Some(_) => self.line(),
            None => self.table.segments.at(self.mark, frame),
        };
        if segment.playing { segment } else { Segment { bpm: self.bpm, frames_per_beat: self.frames_per_beat, ..segment } }
    }

    pub(super) fn segment(&self, back: usize) -> Segment {
        self.segment_of(back, self.clocking())
    }

    fn behind(&self, at: f64, lane: Lane, back: usize, clocking: Clocking) -> (f64, Lane) {
        if back == 0 {
            return (at, lane);
        }
        let segment = self.segment_of(back, clocking);
        let beat = segment.beat_at(self.frame - back as i64);
        (beat, Lane { beats: beat, seconds: beat * 60.0 / segment.bpm })
    }

    fn running(&self, back: usize, clocking: Clocking) -> usize {
        if self.segment_of(back, clocking).playing { back } else { 0 }
    }

    pub(super) fn lag_of(&self, cell: &TrackCell, aim: &Aim, clocking: Clocking) -> usize {
        let depth = depth(&cell.rows, aim, self.sample_rate);
        self.running(peek(&cell.strip).and_then(|strip| strip.plan.as_deref().map(|plan| plan.lag(aim, depth))).unwrap_or(0), clocking)
    }

    pub(super) fn return_lag(&self, index: usize, aim: &Aim) -> usize {
        let depth = self.table.rows(return_host(index)).map_or(0, |rows| depth(rows, aim, self.sample_rate));
        let pick = |latency: usize, lags: &Lags| if *aim == Aim::Gain { latency } else { lags.get(aim.row_or_top()).map_or(0, |lag| lag.of(depth)) };
        self.running(
            match self.table.sends.get(index) {
                Some(cell) => grab(&cell.strip).map_or(0, |ret| pick(ret.plan.latency, &ret.plan.rows)),
                None => pick(self.output, &self.master_rows),
            },
            self.clocking(),
        )
    }

    pub(super) fn automate(&mut self, at: f64, lane: Lane, frames: usize) {
        lane.store(&self.meters.lane);
        for index in 0..self.count {
            let Some(cell) = self.table.tracks.get(index) else {
                break;
            };
            let Some((at, lane, clocking)) = self.clocked(index, at, lane, frames) else {
                continue;
            };
            let curves = self.curves.get(index).map_or(&[][..], Vec::as_slice);
            for curve in curves.iter().filter(|curve| curve.lfo.is_none()) {
                let back = self.lag_of(cell, &curve.aim, clocking);
                let (at, _) = self.behind(at, lane, back, clocking);
                if curve.aim != Aim::Gain {
                    self.set_lane(cell, &curve.aim, curve.at(at));
                } else if let Some(mut strip) = hold(&cell.strip) {
                    let (from, to) = swept(curve, at, self.segment_of(back, clocking), frames);
                    strip.gain.sweep(from, to, frames);
                }
            }
        }
    }

    pub(super) fn wave_row(&mut self, index: usize, row: usize, at: f64, lane: Lane, frames: usize) {
        self.waves(index, Some(row), at, lane, frames);
    }

    pub(super) fn wave(&mut self, at: f64, lane: Lane, frames: usize) {
        for index in 0..self.count {
            self.waves(index, None, at, lane, frames);
        }
    }

    fn waves(&mut self, index: usize, row: Option<usize>, at: f64, lane: Lane, frames: usize) {
        let Some(cell) = self.table.tracks.get(index) else {
            return;
        };
        let Some((at, lane, clocking)) = self.clocked(index, at, lane, frames) else {
            return;
        };
        let curves = self.curves.get(index).map_or(&[][..], Vec::as_slice);
        let notes = self.song.get(index).map_or(&[][..], Vec::as_slice);
        let lfos = self.lfos.get(index).map_or(&[][..], Vec::as_slice);
        let mine = |lfo: &LaneLfo| match (row, lfo.aim.row()) {
            (Some(row), aimed) => aimed == Some(row),
            (None, Some(aimed)) => !cell.rows.get(aimed).is_some_and(Table::is_midi),
            (None, None) => true,
        };
        for (own, lfo) in lfos.iter().enumerate().filter(|(_, lfo)| mine(lfo)) {
            let back = self.lag_of(cell, &lfo.aim, clocking);
            let segment = self.segment_of(back, clocking);
            let (at, lane) = self.behind(at, lane, back, clocking);
            let noted = self.noted(index, cell, lfo, notes, at, back, frames, segment);
            let (sweep, seconds, cycles) = self.swept(index, lfo, lane, curves, at, segment, noted.as_ref().and_then(|(since, _)| *since));
            match lfo.audio_rate() {
                Some(knob) => {
                    let restarts = noted.map_or(Restarts::NONE, |(_, restarts)| restarts);
                    self.bend_lane(cell, lfo, sweep, seconds, cycles, restarts.landings(), knob, frames, segment.frames_per_beat);
                }
                None => {
                    let shares = |other: &LaneLfo| other.audio_rate().is_none() && other.aim == lfo.aim;
                    if lfos[..own].iter().any(&shares) {
                        continue;
                    }
                    let swing = sweep.faded(seconds).swing(cycles)
                        + lfos[own + 1..]
                            .iter()
                            .filter(|other| shares(other))
                            .map(|other| {
                                let noted = self.noted(index, cell, other, notes, at, back, frames, segment).and_then(|(since, _)| since);
                                let (sweep, seconds, cycles) = self.swept(index, other, lane, curves, at, segment, noted);
                                sweep.faded(seconds).swing(cycles)
                            })
                            .sum::<f64>();
                    let travel = (sweep.centre() + swing).clamp(0.0, 1.0);
                    let alpha = lfo.lag_alpha(frames as f64 / self.sample_rate);
                    let travel = self.lagged.get_mut(index).and_then(|lagged| lagged.get_mut(own)).map_or(travel, |held| eased(held, travel, alpha));
                    self.set_lane(cell, &lfo.aim, travel);
                }
            }
        }
    }

    fn noted(&self, index: usize, cell: &TrackCell, lfo: &LaneLfo, notes: &[SongNote], at: f64, back: usize, frames: usize, segment: Segment) -> Option<(Option<Lane>, Restarts)> {
        let aimed = lfo.aim.row_or_top().min(RACK_ROWS - 1);
        let cells = self.meters.sequenced.get(index).and_then(|rows| rows.get(aimed));
        let noted = lfo.restarting().then(|| self.struck(index, cell, &lfo.aim, notes, at, back, frames, segment, lfo.audio_rate().is_none(), lfo.phrased())).flatten();
        if let Some(cells) = cells {
            noted.as_ref().and_then(|(since, _)| *since).unwrap_or(Lane::UNCLOCKED).store(cells);
        }
        noted
    }

    fn struck(&self, index: usize, cell: &TrackCell, aim: &Aim, notes: &[SongNote], at: f64, back: usize, frames: usize, segment: Segment, early: bool, phrased: bool) -> Option<(Option<Lane>, Restarts)> {
        let from = self.frame - back as i64;
        let to = from + frames as i64;
        let (anchor, restarts) = match source(&cell.rows, aim)? {
            Source::Row(row) => {
                let guard = grab(&cell.rows[row])?;
                let Some(Insert::Midi { stream, .. }) = guard.as_ref() else {
                    return None;
                };
                let onsets = stream.onsets();
                let anchor = onsets.within(from, to, phrased).next().filter(|_| early).or_else(|| onsets.latest(from, phrased)).map(|frame| (frame, onsets.drift(frame)));
                let restarts = Restarts::of(onsets.within(from + 1, to, phrased).map(|frame| (frame - from) as f64));
                (anchor, restarts)
            }
            Source::Input => {
                let fpb = segment.frames_per_beat;
                let ahead = at + frames as f64 / fpb;
                let played: &[SongNote] = if segment.playing { notes } else { &[] };
                let (mut latest, mut first) = (None, None);
                for start in struck(played, phrased).take_while(|start| *start < ahead) {
                    if start <= at {
                        latest = Some(start);
                    } else {
                        first = first.or(Some(start));
                    }
                }
                let heard = first.filter(|_| early).or(latest).map(|start| (from - ((at - start) * fpb).round() as i64, 0.0));
                let keyed = self.keys.get(index).copied().flatten().map(|key| (key.frame, key.drift));
                let led = self.leaders.get(index).copied().flatten().and_then(|leader| self.led(leader, from, to, early, phrased));
                let anchor = heard.into_iter().chain(keyed).chain(led).max_by_key(|(frame, _)| *frame);
                let restarts = Restarts::of(struck(played, phrased).map(|start| (start - at) * fpb).skip_while(|landing| *landing < 0.5).take_while(|landing| landing.round() < frames as f64));
                (anchor, restarts)
            }
        };
        let since = anchor.map(|(frame, drift)| ((from - frame).max(0) as f64, drift));
        Some((since.map(|(since, drift)| Lane { beats: since / self.frames_per_beat + drift, seconds: since / self.sample_rate }), restarts))
    }

    fn led(&self, leader: usize, from: i64, to: i64, early: bool, phrased: bool) -> Option<(i64, f64)> {
        let cell = self.table.tracks.get(leader)?;
        let last = (0..RACK_ROWS).rev().find(|&at| Table::is_midi(&cell.rows[at]))?;
        let guard = grab(&cell.rows[last])?;
        let Some(Insert::Midi { stream, .. }) = guard.as_ref() else {
            return None;
        };
        let onsets = stream.onsets();
        onsets.within(from, to, phrased).next().filter(|_| early).or_else(|| onsets.latest(from, phrased)).map(|frame| (frame, onsets.drift(frame)))
    }

    fn swept(&self, index: usize, lfo: &LaneLfo, lane: Lane, curves: &[Curve], at: f64, segment: Segment, noted: Option<Lane>) -> (Sweep, f64, f64) {
        let since = noted.unwrap_or(lane);
        let cycles = if noted.is_none() && segment.playing { since.beats * lfo.cycles_per_beat(segment.bpm) } else { lfo.cycles_elapsed(since.beats, since.seconds) };
        let share = self.table.tracks.get(index).map_or(1.0, |cell| lane_share(&cell.rows, &lfo.aim));
        (lfo.sweep(curves, at, share), since.seconds, cycles)
    }

    fn bend_lane(&self, cell: &TrackCell, lfo: &LaneLfo, sweep: Sweep, seconds: f64, cycles: f64, restarts: &[f64], (osc, unit): (usize, f64), frames: usize, frames_per_beat: f64) {
        let Aim::Knob { row, band, name } = &lfo.aim else {
            return;
        };
        let Some(row) = cell.rows.get(*row) else {
            return;
        };
        let Some(mut row) = grab(row) else {
            return;
        };
        let Some(Insert::Core { layer, core, .. }) = row.as_mut() else {
            return;
        };
        let Some(knob) = sdk::param::find(coresynth::params::PARAMS, band, name) else {
            return;
        };
        let held = (knob.get)(&layer.patch);
        let step = lfo.cycles_per_frame(frames_per_beat, self.sample_rate);
        let mut restarts = restarts.iter().copied().peekable();
        let mut restarted = None;
        let mut semitones = [0.0; BEND_FRAMES];
        let frames = frames.min(BEND_FRAMES);
        for (frame, bent) in semitones[..frames].iter_mut().enumerate() {
            let frame = frame as f64;
            while let Some(landing) = restarts.next_if(|landing| landing.round() <= frame) {
                restarted = Some(landing);
            }
            let turned = restarted.map_or(cycles + frame * step, |start| (frame - start) * step);
            let sweep = sweep.faded(restarted.map_or(seconds + frame / self.sample_rate, |start| (frame - start) / self.sample_rate));
            *bent = (knob.scale.value(sweep.travel(turned)) - held) * unit;
        }
        core.bend(osc, &semitones[..frames]);
    }

    pub(super) fn set_lane(&self, cell: &TrackCell, aim: &Aim, travel: f64) {
        match aim {
            Aim::Gain => {
                if let Some(mut strip) = hold(&cell.strip) {
                    strip.gain.set(position_to_gain(travel as f32));
                }
            }
            Aim::Send(send) => {
                if let Some(mut strip) = hold(&cell.strip)
                    && let Some(level) = strip.sends.get_mut(*send)
                {
                    *level = travel as f32;
                }
            }
            Aim::Level(_) | Aim::Wet(_) | Aim::Knob { .. } => lane_row(&cell.rows, aim, travel),
        }
    }

    pub(super) fn automate_returns(&mut self, at: f64, lane: Lane, frames: usize) {
        let mut master = None;
        let seconds = frames as f64 / self.sample_rate;
        let mut lagged = std::mem::take(&mut self.return_lagged);
        for index in 0..RETURNS {
            let host = return_host(index);
            let curves = self.return_curves.get(index).map_or(&[][..], Vec::as_slice);
            let direct = curves.iter().filter(|curve| curve.lfo.is_none()).map(|curve| {
                let back = self.return_lag(index, &curve.aim);
                let (at, _) = self.behind(at, lane, back, self.clocking());
                let (from, to) = if curve.aim == Aim::Gain { swept(curve, at, self.segment(back), frames) } else { (0.0, 0.0) };
                (&curve.aim, curve.at(at), from, to)
            });
            let mut held = lagged.get_mut(index).into_iter().flatten();
            let waved = self.return_lfos.get(index).map_or(&[][..], Vec::as_slice).iter().map(|lfo| {
                let back = self.return_lag(index, &lfo.aim);
                let segment = self.segment(back);
                let (at, lane) = self.behind(at, lane, back, self.clocking());
                let cycles = if segment.playing { lane.beats * lfo.cycles_per_beat(segment.bpm) } else { lfo.cycles_elapsed(lane.beats, lane.seconds) };
                let share = self.table.rows(host).map_or(1.0, |rows| lane_share(rows, &lfo.aim));
                let travel = lfo.travel_cycles(curves, at, cycles, lane.seconds, share);
                let travel = held.next().map_or(travel, |held| eased(held, travel, lfo.lag_alpha(seconds)));
                let gain = position_to_gain(travel as f32);
                (&lfo.aim, travel, gain, gain)
            });
            for (aim, travel, from, to) in direct.chain(waved) {
                if *aim != Aim::Gain {
                    if let Some(rows) = self.table.rows(host) {
                        lane_row(rows, aim, travel);
                    }
                    continue;
                }
                match host {
                    Host::Send(send) => {
                        if let Some(mut ret) = self.table.sends.get(send).and_then(|cell| grab(&cell.strip)) {
                            ret.gain.sweep(from, to, frames);
                        }
                    }
                    _ => master = Some((from, to)),
                }
            }
        }
        self.return_lagged = lagged;
        if let Some((from, to)) = master {
            self.master.sweep(from, to, frames);
        }
    }
}

fn eased(held: &mut Option<f64>, travel: f64, alpha: f64) -> f64 {
    let eased = held.map_or(travel, |held| held + (travel - held) * alpha);
    *held = Some(eased);
    eased
}

fn depth(rows: &Rows, aim: &Aim, sample_rate: f64) -> usize {
    let Some(row) = rows.get(aim.row_or_top()).and_then(grab) else {
        return 0;
    };
    match (row.as_ref(), aim) {
        (Some(Insert::Midi { .. } | Insert::Core { .. } | Insert::Bus { .. }) | None, _) => 0,
        (Some(Insert::Effect { slot, .. }), Aim::Level(_) | Aim::Wet(_)) => slot.mix_depth(sample_rate),
        (Some(Insert::Effect { slot, .. }), Aim::Knob { band, name, .. }) => slot.depth(band, name, sample_rate),
        (Some(Insert::Effect { slot, .. }), Aim::Gain | Aim::Send(_)) => slot.depth("", "", sample_rate),
    }
}

fn lane_share(rows: &Rows, aim: &Aim) -> f64 {
    let Aim::Knob { row, band, name } = aim else {
        return 1.0;
    };
    let Some(row) = rows.get(*row).and_then(grab) else {
        return 1.0;
    };
    match row.as_ref() {
        Some(Insert::Midi { .. }) => MidiRow::lane_share(band, name),
        Some(Insert::Core { .. }) => coresynth::Params::lane_share(band, name),
        Some(Insert::Effect { slot, .. }) => slot.params.lane_share(band, name),
        Some(Insert::Bus { .. }) | None => 1.0,
    }
}

fn lane_row(rows: &Rows, aim: &Aim, travel: f64) {
    let Some(mut row) = aim.row().and_then(|at| rows.get(at)).and_then(grab) else {
        return;
    };
    match (row.as_mut(), aim) {
        (Some(Insert::Midi { row, runner, .. }), Aim::Knob { band, name, .. }) => {
            if row.set_travel(band, name, travel) {
                runner.stage(row.heard());
            }
        }
        (Some(Insert::Core { layer, .. }), Aim::Level(_)) => layer.level = travel,
        (Some(Insert::Core { layer, core, .. }), Aim::Knob { band, name, .. }) => {
            if layer.patch.set_travel(band, name, travel) {
                core.set_params(layer.patch);
            }
        }
        (Some(Insert::Effect { slot, .. }), Aim::Wet(_)) => slot.wet = travel,
        (Some(Insert::Effect { slot, .. }), Aim::Knob { band, name, .. }) => {
            slot.params.set_travel(band, name, travel);
        }
        _ => {}
    }
}

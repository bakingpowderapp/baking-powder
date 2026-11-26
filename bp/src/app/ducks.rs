use super::*;
use super::automation::{AUTO_LINE, Armed, auto_y};
use super::files::{Exporting, FLATTENING, Rendering};
use super::view::return_name;
use crate::audio::duck::{self, Automated, Trace, Tracing};
use crate::project::fitted;
use fx::params::Sidechain;

const HALO: i32 = 1;
const FLATTEN: &str = "FLATTEN";
const LOCKED: &str = "LANES LOCKED";
const DEEPEST_SECONDS: f32 = 3.0;

enum Sought {
    Rendering(Tracing),
    Found(Option<Arc<Trace>>),
}

struct Signal {
    recipe: Project,
    end: i32,
    waves: Vec<Arc<Wave>>,
    users: Vec<(usize, usize)>,
    sought: Sought,
    last: Option<Arc<Trace>>,
}

impl Signal {
    fn renders(&self, recipe: &Project, end: i32, waves: &[Arc<Wave>]) -> bool {
        self.end == end && self.recipe == *recipe && same(&self.waves, waves)
    }

    fn trace(&self) -> Option<&Arc<Trace>> {
        match &self.sought {
            Sought::Rendering(_) => self.last.as_ref(),
            Sought::Found(trace) => trace.as_ref(),
        }
    }
}

struct Ducked {
    track: usize,
    row: usize,
    automated: Automated,
    trace: Arc<Trace>,
    gains: Arc<Vec<f32>>,
}

#[derive(Default)]
pub(super) struct Ducks {
    signals: Vec<Signal>,
    ducked: Vec<Ducked>,
    seen: Option<(u64, Vec<Arc<Wave>>)>,
    sent: Option<(u64, Arc<flatten::Sent>)>,
    watched: Option<(Host, usize)>,
    deepest: f32,
    age: f32,
}

impl Ducks {
    pub(super) fn deepest(&self) -> f32 {
        self.deepest
    }
}

struct Volume<'a> {
    wave: Option<LaneLfo>,
    curves: &'a [Curve],
    notes: Vec<SongNote>,
    bpm: f64,
    fader: f32,
}

impl Volume<'_> {
    fn gain(&self, beat: f64) -> f32 {
        let travel = match &self.wave {
            Some(lfo) => lfo.travel(self.curves, &self.notes, beat, self.bpm, 1.0),
            None => self.curves.iter().find(|curve| curve.lfo.is_none() && curve.aim == Aim::Gain).map_or(f64::from(self.fader), |curve| curve.at(beat)),
        };
        position_to_gain(travel as f32)
    }
}

fn same(mine: &[Arc<Wave>], theirs: &[Arc<Wave>]) -> bool {
    mine.len() == theirs.len() && mine.iter().zip(theirs).all(|(mine, theirs)| Arc::ptr_eq(mine, theirs))
}

fn used(recipe: &Project, waves: &[Arc<Wave>]) -> Vec<Arc<Wave>> {
    let named = |wave: &Arc<Wave>| recipe.tracks.iter().flat_map(|track| &track.clips).any(|clip| clip.file.as_ref().is_some_and(|file| file.name == wave.name));
    waves.iter().filter(|wave| named(wave)).cloned().collect()
}

fn signalled(row: &Row) -> Option<(Slot, Sidechain, usize, bool)> {
    match row {
        Row::Effect(slot @ Slot { params: fx::Params::Sidechain(params), .. }) => slot.source().map(|(source, pre)| (*slot, *params, source, pre)),
        _ => None,
    }
}

fn previewed(row: &Row) -> Option<(Slot, Sidechain, usize, bool)> {
    signalled(row).filter(|(_, params, ..)| params.preview)
}

fn ducking(row: &Row) -> Option<(Slot, Sidechain, usize, bool)> {
    previewed(row).filter(|(slot, ..)| !slot.bypass)
}

impl App {
    pub(super) fn tick_deepest(&mut self, seconds: f32) -> bool {
        let watched = self.shown();
        let now = watched.zip(self.audio.as_ref()).map_or(0.0, |((host, row), audio)| audio.gauge(host, row));
        let held = &mut self.ducks;
        let was = held.deepest;
        if held.watched != watched || now >= held.deepest || held.age >= DEEPEST_SECONDS {
            (held.watched, held.deepest, held.age) = (watched, now, 0.0);
        } else {
            held.age += seconds;
        }
        held.deepest != was
    }

    pub(super) fn tick_ducks(&mut self) -> bool {
        let mut landed = false;
        for signal in &mut self.ducks.signals {
            if let Sought::Rendering(tracing) = &mut signal.sought
                && tracing.finished()
            {
                signal.sought = Sought::Found(tracing.take().map(Arc::new));
                landed = true;
            }
        }
        let stamp = self.history.changes();
        if self.ducks.seen.as_ref().is_some_and(|(seen, waves)| *seen == stamp && same(waves, &self.pool.waves)) {
            return landed;
        }
        self.ducks.seen = Some((stamp, self.pool.waves.clone()));
        let end = self.project.end();
        let mut old = std::mem::take(&mut self.ducks.signals);
        let drawn = !old.is_empty();
        let shown: Vec<((usize, usize), Arc<Trace>)> = old.iter().flat_map(|signal| signal.users.iter().filter_map(|user| signal.trace().map(|trace| (*user, Arc::clone(trace))))).collect();
        let mut signals: Vec<Signal> = Vec::new();
        for (track, owner) in self.project.tracks.iter().enumerate() {
            for (row, (_, _, source, pre)) in owner.rack.iter().enumerate().filter_map(|(row, entry)| ducking(entry).map(|found| (row, found))) {
                let recipe = duck::recipe(&self.project, Host::Track(track), source, pre);
                let waves = used(&recipe, &self.pool.waves);
                if let Some(signal) = signals.iter_mut().find(|signal| signal.renders(&recipe, end, &waves)) {
                    signal.users.push((track, row));
                    continue;
                }
                let kept = old.iter().position(|signal| signal.renders(&recipe, end, &waves)).map(|at| old.swap_remove(at));
                let last = shown.iter().find(|(user, _)| *user == (track, row)).map(|(_, trace)| Arc::clone(trace));
                let sought = kept.map_or_else(|| Trace::start(&recipe, &waves, end).map_or(Sought::Found(None), Sought::Rendering), |signal| signal.sought);
                signals.push(Signal { recipe, end, waves, users: vec![(track, row)], sought, last });
            }
        }
        for signal in old {
            if let Sought::Rendering(tracing) = signal.sought {
                tracing.cancel();
            }
        }
        self.ducks.ducked.retain(|ducked| signals.iter().any(|signal| signal.users.contains(&(ducked.track, ducked.row))));
        self.ducks.signals = signals;
        landed || drawn || !self.ducks.signals.is_empty()
    }

    fn duck_notes(&mut self, host: Host, row: usize) -> Vec<SongNote> {
        let Some(track) = host.track().filter(|_| Automated::restarts(&self.project, host, row)) else {
            return Vec::new();
        };
        let stamp = self.history.changes();
        let sent = match &self.ducks.sent {
            Some((seen, sent)) if *seen == stamp => Arc::clone(sent),
            _ => {
                let sent = Arc::new(audio::sent(&self.project));
                self.ducks.sent = Some((stamp, Arc::clone(&sent)));
                sent
            }
        };
        sent.heard(&self.project, track, &Aim::Wet(row))
    }

    fn volume(&self, host: Host) -> Volume<'_> {
        let wave = self.lane_wave(host, &Armed::of(Aim::Gain));
        let notes = if wave.is_some() { self.project.song_notes_of(host) } else { Vec::new() };
        Volume { wave, curves: &self.project.automation(host).curves, notes, bpm: self.project.bpm, fader: gain_to_position(self.project.gain_of(host)) }
    }

    fn ducks_of(&mut self, track: usize) -> Option<Vec<Arc<Vec<f32>>>> {
        let rack = &self.project.tracks[track].rack;
        if !rack.iter().any(|entry| previewed(entry).is_some()) {
            return None;
        }
        let rows: Vec<usize> = rack.iter().enumerate().filter(|(_, entry)| ducking(entry).is_some()).map(|(row, _)| row).collect();
        let mut ducks = Vec::with_capacity(rows.len());
        for row in rows {
            let Some(trace) = self.ducks.signals.iter().find(|signal| signal.users.contains(&(track, row))).and_then(Signal::trace).cloned() else {
                continue;
            };
            let notes = self.duck_notes(Host::Track(track), row);
            let Some(automated) = Automated::of(&self.project, Host::Track(track), row, notes) else {
                continue;
            };
            let at = self.ducks.ducked.iter().position(|ducked| (ducked.track, ducked.row) == (track, row));
            let held = at.map(|at| &self.ducks.ducked[at]).filter(|ducked| ducked.automated == automated && Arc::ptr_eq(&ducked.trace, &trace));
            let gains = match held {
                Some(ducked) => Arc::clone(&ducked.gains),
                None => {
                    let gains = Arc::new(trace.gains(&automated));
                    let ducked = Ducked { track, row, automated, trace, gains: Arc::clone(&gains) };
                    match at {
                        Some(at) => self.ducks.ducked[at] = ducked,
                        None => self.ducks.ducked.push(ducked),
                    }
                    gains
                }
            };
            ducks.push(gains);
        }
        Some(ducks)
    }

    pub(super) fn draw_duck(&mut self, track: usize, lane: Lane) {
        let Some(ducks) = self.ducks_of(track) else {
            return;
        };
        let per_beat = duck::blocks_per_beat(self.project.bpm);
        let volume = self.volume(Host::Track(track));
        let mut runs: Vec<Rect> = Vec::new();
        let mut entered = volume.gain(lane.beats_at(lane.rect.x));
        for x in lane.rect.x..lane.rect.x + lane.rect.w {
            let (from, to) = (lane.beats_at(x), lane.beats_at(x + 1));
            let left = volume.gain(to);
            let (quiet, loud) = (entered.min(left), entered.max(left));
            entered = left;
            let blocks = (from * per_beat).floor().max(0.0) as usize..=(to * per_beat).ceil().max(0.0) as usize;
            let (low, high) = blocks.fold((f32::MAX, f32::MIN), |(low, high), block| {
                let gain: f32 = ducks.iter().map(|gains| gains.get(block).copied().unwrap_or(1.0)).product();
                (low.min(gain), high.max(gain))
            });
            let (top, bottom) = (auto_y(lane, gain_to_position(loud * high)), auto_y(lane, gain_to_position(quiet * low)));
            match runs.last_mut() {
                Some(run) if (run.y, run.h) == (top, bottom - top + AUTO_LINE) => run.w += 1,
                _ => runs.push(Rect::new(x, top, 1, bottom - top + AUTO_LINE)),
            }
        }
        for run in &runs {
            self.list.fill(Rect::new(run.x, run.y - HALO, run.w, run.h + 2 * HALO), Tone::Ground);
        }
        for run in runs {
            self.list.fill(run, Tone::Ink100);
        }
    }

    pub(super) fn flatten_duck(&mut self, host: Host, row: usize) {
        let Some((_, _, source, pre)) = self.project.rack(host).get(row).and_then(signalled) else {
            return;
        };
        let name = host.track().map_or_else(|| return_name(host), |track| self.project.tracks[track].name.clone());
        if self.project.lock {
            self.notice = Some(Notice { band: FLATTEN, name, reason: LOCKED });
            return;
        }
        let recipe = duck::recipe(&self.project, host, source, pre);
        let (end, waves) = (self.project.end(), used(&recipe, &self.pool.waves));
        let found = self.ducks.signals.iter().filter(|signal| signal.renders(&recipe, end, &waves)).find_map(|signal| match &signal.sought {
            Sought::Rendering(_) => None,
            Sought::Found(trace) => trace.clone(),
        });
        if let Some(trace) = found {
            self.land_duck(host, row, &trace);
            return;
        }
        let Some(tracing) = Trace::start(&recipe, &waves, end) else {
            return;
        };
        if self.playing {
            self.stop();
        }
        self.disk.exporting = Some(Exporting { rendering: Rendering::Signal(host, row, tracing), name, title: String::from(FLATTENING), age: 0.0 });
    }

    pub(super) fn land_duck(&mut self, host: Host, row: usize, trace: &Trace) {
        let notes = self.duck_notes(host, row);
        let Some(automated) = self.project.rack(host).get(row).and_then(signalled).and_then(|_| Automated::of(&self.project, host, row, notes)) else {
            return;
        };
        let gains = trace.gains(&automated);
        if gains.iter().any(|gain| *gain < 1.0) {
            let per_beat = duck::blocks_per_beat(self.project.bpm);
            let volume = self.volume(host);
            let mut points: Vec<(i32, f32)> = Vec::with_capacity(gains.len());
            for (block, gain) in gains.iter().enumerate() {
                let beat = block as f64 / per_beat;
                let (tick, travel) = (ticks(beat), gain_to_position(volume.gain(beat) * gain).clamp(0.0, 1.0));
                match points.last_mut() {
                    Some(last) if last.0 == tick => last.1 = last.1.min(travel),
                    _ => points.push((tick, travel)),
                }
            }
            self.project.automation_mut(host).print(&Aim::Gain, fitted(&points));
        }
        self.project.rack_mut(host)[row].set_bypass(true);
        self.armed = Some(Armed::of(Aim::Gain));
        self.reveal_armed();
    }
}

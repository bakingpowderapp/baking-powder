use super::*;
use crate::project::{Automation, Curve, Return, beats};
use std::collections::BTreeMap;
use std::sync::mpsc::{Sender, TryRecvError, channel};

const BLOCK: usize = 256;
const PASS_BLOCK: usize = 4096;
const SETTLE_BLOCKS: usize = 4;
const TAIL_SECONDS: f64 = 2.0;
const SILENCE: f32 = 1e-6;
const LEVEL_SECONDS: f64 = 0.01;
const MEMO_FRAMES: usize = 32 << 20;

#[derive(Clone)]
pub struct Vision {
    frames_per_beat: f64,
    offset: usize,
    l: Vec<f32>,
    r: Vec<f32>,
    level: f32,
    smoothing: f32,
}

impl Vision {
    pub fn resume(&mut self, from: &Vision) {
        self.level = from.level;
    }

    pub fn lay(&mut self, beat: f64, l: &mut [f32], r: &mut [f32], level: f32) {
        let mut held = self.level;
        self.lay_with(&mut held, beat, l, r, level);
        self.level = held;
    }

    pub fn lay_with(&self, held: &mut f32, beat: f64, l: &mut [f32], r: &mut [f32], level: f32) {
        if level == 0.0 && *held < SILENCE {
            *held = 0.0;
            return;
        }
        let start = (beat * self.frames_per_beat).round() as i64 - self.offset as i64;
        for (i, (a, b)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
            let at = usize::try_from(start + i as i64).ok();
            let (fl, fr) = at.and_then(|at| self.l.get(at).zip(self.r.get(at))).map_or((0.0, 0.0), |(fl, fr)| (*fl, *fr));
            *held += self.smoothing * (level - *held);
            *a += *held * fl;
            *b += *held * fr;
        }
    }
}

#[derive(Clone)]
pub struct Capture {
    l: Vec<f32>,
    r: Vec<f32>,
}

impl Capture {
    pub(super) fn new(frames: usize) -> Capture {
        Capture { l: Vec::with_capacity(frames), r: Vec::with_capacity(frames) }
    }

    pub(super) fn record(&mut self, l: &[f32], r: &[f32]) {
        self.l.extend_from_slice(l);
        self.r.extend_from_slice(r);
    }

    fn frames(&self) -> usize {
        self.l.len()
    }

    fn scale(&mut self, factor: f32) {
        if factor == 1.0 {
            return;
        }
        for sample in self.l.iter_mut().chain(&mut self.r) {
            *sample *= factor;
        }
    }

    fn into_vision(mut self, frames_per_beat: f64, sample_rate: f64, upstream: usize) -> Vision {
        for channel in [&mut self.l, &mut self.r] {
            channel.drain(..upstream.min(channel.len()));
        }
        let loud = |(a, b): (&f32, &f32)| a.abs() > SILENCE || b.abs() > SILENCE;
        let first = self.l.iter().zip(&self.r).position(loud).unwrap_or(self.l.len());
        let end = self.l.iter().zip(&self.r).rposition(loud).map_or(first, |last| last + 1);
        for channel in [&mut self.l, &mut self.r] {
            channel.truncate(end);
            channel.drain(..first.min(end));
            channel.shrink_to_fit();
        }
        Vision { frames_per_beat, offset: first, l: self.l, r: self.r, level: 0.0, smoothing: sdk::dsp::smoothing(LEVEL_SECONDS, sample_rate) as f32 }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Blank,
    Host,
    Whole,
}

#[derive(Clone)]
pub struct Recipe {
    project: Project,
    solo: Option<usize>,
    waves: Vec<Arc<Wave>>,
    rows: Vec<(Host, usize)>,
    scale: [f32; SEND_COUNT],
}

impl PartialEq for Recipe {
    fn eq(&self, other: &Recipe) -> bool {
        self.project == other.project && self.solo == other.solo && self.rows == other.rows && self.scale == other.scale && same_waves(&self.waves, &other.waves)
    }
}

fn same_waves(mine: &[Arc<Wave>], theirs: &[Arc<Wave>]) -> bool {
    mine.len() == theirs.len() && mine.iter().zip(theirs).all(|(mine, theirs)| Arc::ptr_eq(mine, theirs))
}

fn reaches(project: &Project, track: usize, host: Host) -> bool {
    let Host::Send(send) = host else {
        return true;
    };
    let sends = |track: usize| fed_send(&project.tracks[track], send);
    sends(track) || project.group_of(track).is_some_and(sends) || signals(&project.sends[send].rack).any(|source| source == track)
}

fn automated(track: &Track, aim: &Aim) -> bool {
    track.automation.lfo(aim).is_some() || track.automation.curves.iter().any(|curve| curve.aim == *aim)
}

fn fed_send(track: &Track, send: usize) -> bool {
    track.sends[send] > 0.0 || automated(track, &Aim::Send(send))
}

fn soles(project: &Project, rows: &[(Host, usize)]) -> Vec<(usize, usize)> {
    (0..SEND_COUNT)
        .filter_map(|send| {
            let alone = rows.iter().filter(|(host, _)| *host == Host::Send(send)).map(|(_, index)| *index).eq(std::iter::once(0));
            let mut feeders = (0..project.tracks.len()).filter(|&track| fed_send(&project.tracks[track], send));
            let track = feeders.next()?;
            (alone && feeders.next().is_none() && !automated(&project.tracks[track], &Aim::Send(send))).then_some((send, track))
        })
        .collect()
}

fn reversed_rows<'a>(rack: &'a [Row], automation: &'a Automation) -> impl Iterator<Item = usize> + 'a {
    rack.iter().enumerate().filter(move |(index, row)| {
        let Row::Effect(slot) = row else {
            return false;
        };
        if slot.reverse_level() > 0.0 {
            return true;
        }
        let Some(band) = slot.reverse_band() else {
            return false;
        };
        let reverse = Aim::knob(*index, band, "REVERSE");
        automation.lfo(&reverse).is_some()
            || automation.curves.iter().any(|curve| {
                curve.aim == reverse
                    && if curve.lfo.is_some() { !curve.nodes.is_empty() } else { curve.nodes.iter().any(|node| node.travel > 0.0) }
            })
    }).map(|(index, _)| index)
}

fn hosts(tracks: usize) -> impl Iterator<Item = Host> {
    (0..tracks).map(Host::Track).chain((0..SEND_COUNT).map(Host::Send)).chain(std::iter::once(Host::Master))
}

fn signals(rack: &[Row]) -> impl Iterator<Item = usize> + '_ {
    rack.iter().filter_map(|row| match row {
        Row::Effect(slot) => slot.source().map(|(track, _)| track),
        _ => None,
    })
}

fn kept(track: &Track) -> usize {
    reversed_rows(&track.rack, &track.automation).last().map_or(0, |last| last + 1)
}

pub(super) fn feeders(project: &Project, track: usize, keep: usize) -> Vec<usize> {
    let fed = project.fed_by(track);
    let mut feeders: Vec<usize> = (0..project.tracks.len()).filter(|&source| fed & (1 << source) != 0).collect();
    feeders.extend(project.members_of(track));
    feeders.extend(signals(&project.tracks[track].rack[..keep]));
    feeders
}

fn settle_last(rack: &mut [Row]) -> Option<&'static str> {
    if let Some(Row::Effect(slot)) = rack.last_mut()
        && let Some(band) = slot.reverse_band()
    {
        *slot = fx::Slot { wet: 1.0, bypass: false, ..slot.blended(0.0, 1.0) };
        return Some(band);
    }
    None
}

fn cut(ret: &mut Return) {
    let kept = reversed_rows(&ret.rack, &ret.automation).last().map_or(0, |last| last + 1);
    ret.rack.truncate(kept);
    if let Some(band) = settle_last(&mut ret.rack) {
        let last = kept - 1;
        remove_levels(&mut ret.automation, |aim| reversing(aim, last, band));
    }
}

fn unvolumed(ret: &Return) -> Return {
    let mut flat = Return { gain: 1.0, ..ret.clone() };
    remove_levels(&mut flat.automation, |aim| *aim == Aim::Gain);
    flat
}

fn trim(track: &mut Track) {
    let keep = kept(track);
    track.rack.truncate(keep);
    if let Some(band) = settle_last(&mut track.rack) {
        let last = keep - 1;
        remove_levels(&mut track.automation, |aim| reversing(aim, last, band));
    }
}

fn reversing(aim: &Aim, last: usize, reverse_band: &str) -> bool {
    aim.row() == Some(last) && !matches!(aim, Aim::Knob { band, name, .. } if band != reverse_band || !matches!(name.as_str(), "FORWARD" | "REVERSE"))
}

pub(super) fn remove_levels(automation: &mut Automation, output: impl Fn(&Aim) -> bool) {
    automation.lfos.retain(|lfo| !output(&lfo.aim));
    automation.curves.retain(|curve| !output(&curve.aim));
}

fn reduce(track: &Track, role: Role, returns: &[bool; SEND_COUNT]) -> Track {
    match role {
        Role::Blank => Track { audio: track.audio, group: track.group, inside: track.inside, solo: track.solo, mute: track.mute, ..Track::audio() },
        Role::Host => {
            let mut reduced = Track { name: String::new(), gain: 1.0, delay_ms: 0.0, sends: [0.0; SEND_COUNT], folded: false, ..track.clone() };
            remove_levels(&mut reduced.automation, |aim| matches!(aim, Aim::Send(_)));
            trim(&mut reduced);
            reduced
        }
        Role::Whole => {
            let mut reduced = Track { name: String::new(), sends: std::array::from_fn(|send| if returns[send] { track.sends[send] } else { 0.0 }), folded: false, ..track.clone() };
            remove_levels(&mut reduced.automation, |aim| matches!(aim, Aim::Send(send) if returns.get(*send) != Some(&true)));
            reduced
        }
    }
}

pub fn unforeseen<'a>(project: &'a Project, recipe: Option<&'a Recipe>) -> impl Iterator<Item = (Host, usize)> + 'a {
    hosts(project.tracks.len()).flat_map(move |host| {
        let rows = project.rack(host).iter().enumerate();
        rows.filter(move |(index, row)| matches!(row, Row::Effect(slot) if slot.reverse_band().is_some()) && recipe.is_none_or(|recipe| !recipe.rows.contains(&(host, *index)))).map(move |(index, _)| (host, index))
    })
}

pub fn recipe(project: &Project, solo: Option<usize>, waves: &[Arc<Wave>], hosted: impl Fn(Host) -> bool) -> Option<Recipe> {
    let count = project.tracks.len();
    let rows: Vec<(Host, usize)> = hosts(count).filter(|&host| hosted(host)).flat_map(|host| reversed_rows(project.rack(host), project.automation(host)).map(move |row| (host, row))).collect();
    if rows.is_empty() {
        return None;
    }
    let deep = rows.iter().any(|(host, _)| host.track().is_none());
    let master = rows.iter().any(|(host, _)| *host == Host::Master);
    let returns: [bool; SEND_COUNT] = std::array::from_fn(|send| master || rows.iter().any(|(host, _)| *host == Host::Send(send)));
    let mut roles = vec![if master { Role::Whole } else { Role::Blank }; count];
    let mut queue = Vec::new();
    if !master {
        for &(host, _) in &rows {
            match host.track() {
                Some(track) => {
                    roles[track] = Role::Host;
                    queue.extend(feeders(project, track, kept(&project.tracks[track])));
                }
                None => queue.extend((0..count).filter(|&track| reaches(project, track, host))),
            }
        }
    }
    while let Some(track) = queue.pop() {
        if track >= count || roles[track] == Role::Whole {
            continue;
        }
        roles[track] = Role::Whole;
        queue.extend(feeders(project, track, project.tracks[track].rack.len()));
    }
    let heard = heard(project);
    let mut reduced = Project::new();
    reduced.tracks = project.tracks.iter().enumerate().map(|(track, original)| reduce(original, roles[track], &returns)).collect();
    reduced.bpm = project.bpm;
    if reduced.tracks.iter().flat_map(|track| &track.rack).any(|row| matches!(row, Row::Midi(midi) if midi.force_key)) {
        (reduced.root, reduced.scale) = (project.root, project.scale);
    }
    if deep {
        reduced.master = unvolumed(&project.master);
        reduced.sends = std::array::from_fn(|send| if master { project.sends[send].clone() } else { unvolumed(&project.sends[send]) });
        cut(&mut reduced.master);
        if !master {
            for send in &mut reduced.sends {
                cut(send);
            }
        }
    }
    let soles = if master { Vec::new() } else { soles(project, &rows) };
    let sidechained = |track: usize| hosts(count).any(|host| signals(project.rack(host)).any(|source| source == track));
    let mut scale = [1.0; SEND_COUNT];
    for &(send, track) in &soles {
        let original = &project.tracks[track];
        let alone = project.group_of(track).is_none() && !sidechained(track) && (0..SEND_COUNT).filter(|&other| returns[other] && fed_send(original, other)).all(|other| soles.contains(&(other, track)));
        let gain = alone && !automated(original, &Aim::Gain);
        let mute = alone && project.members_of(track).is_empty();
        scale[send] = original.sends[send] * if gain { original.gain } else { 1.0 } * if mute && !heard[track] { 0.0 } else { 1.0 };
        let track = &mut reduced.tracks[track];
        track.sends[send] = 1.0;
        if gain {
            track.gain = 1.0;
        }
        if mute {
            track.mute = false;
        }
    }
    let waves = waves.iter().filter(|wave| reduced.tracks.iter().flat_map(|track| &track.clips).any(|clip| clip.file.as_ref().is_some_and(|file| file.name == wave.name))).cloned().collect();
    Some(Recipe { project: reduced, solo: solo.filter(|_| deep), waves, rows, scale })
}

pub struct Rendered {
    pub host: Host,
    pub index: usize,
    pub vision: Box<Vision>,
}

fn feeds(project: &Project, (from, at): (Host, usize), (to, index): (Host, usize)) -> bool {
    if from == to {
        return at < index;
    }
    match (from.track(), to.track()) {
        (Some(a), Some(b)) if a == b => false,
        (Some(a), Some(b)) => project.members_of(b).contains(&a) || signals(&project.tracks[b].rack).any(|source| source == a),
        (Some(a), None) => reaches(project, a, to),
        (None, Some(_)) => false,
        (None, None) => matches!((from, to), (Host::Send(_), Host::Master)),
    }
}

fn rounds(project: &Project, rows: &[(Host, usize)]) -> Vec<Vec<(Host, usize)>> {
    let mut depth = vec![0; rows.len()];
    for _ in 0..rows.len() {
        let mut settled = true;
        for (i, &to) in rows.iter().enumerate() {
            let below = rows.iter().enumerate().filter(|&(j, &from)| j != i && feeds(project, from, to)).map(|(j, _)| depth[j] + 1).max().unwrap_or(0);
            if below > depth[i] {
                depth[i] = below;
                settled = false;
            }
        }
        if settled {
            break;
        }
    }
    let mut rounds: BTreeMap<usize, Vec<(Host, usize)>> = BTreeMap::new();
    for (&row, &at) in rows.iter().zip(&depth) {
        rounds.entry(at).or_default().push(row);
    }
    rounds.into_values().collect()
}

struct Memo {
    sample_rate: f64,
    solo: Option<usize>,
    waves: Vec<Arc<Wave>>,
    rounds: Vec<Vec<(Host, usize)>>,
    keys: Vec<Project>,
    captures: Vec<Vec<Arc<Capture>>>,
}

impl Memo {
    fn recalls(&self, sample_rate: f64, recipe: &Recipe, rounds: &[Vec<(Host, usize)>], key: &Project, round: usize) -> bool {
        self.sample_rate == sample_rate && self.solo == recipe.solo && same_waves(&self.waves, &recipe.waves) && self.rounds == rounds && self.keys[round] == *key
    }
}

fn keyed(project: &Project, rounds: &[Vec<(Host, usize)>]) -> Project {
    let mut key = project.clone();
    for &(host, index) in rounds.iter().flatten() {
        if let Some(Row::Effect(slot)) = key.rack_mut(host).get_mut(index) {
            *slot = fx::Slot::new(slot.kind());
        }
        remove_levels(key.automation_mut(host), |aim| aim.row_or_top() == index);
    }
    key
}

fn helpers() -> usize {
    std::thread::available_parallelism().map_or(0, |cores| (cores.get() / 2).saturating_sub(1))
}

fn render(recipe: &Recipe, sample_rate: f64, progress: &Progress, memo: &mut Option<Memo>, mut preempted: impl FnMut() -> bool) -> Option<Vec<Rendered>> {
    let project = &recipe.project;
    let frames_per_beat = frames_per_beat(sample_rate, project.bpm);
    let song = (frames_per_beat * beats(project.end())).round() as usize;
    let total = song + (sample_rate * TAIL_SECONDS).round() as usize;
    let clock = Clock { sample_rate, bpm: project.bpm, beat: None };
    let rounds = rounds(project, &recipe.rows);
    let timing = Timing::of(project, sample_rate);
    let keys: Vec<Project> = (0..rounds.len()).map(|round| keyed(project, &rounds[round..])).collect();
    let recalled: Vec<bool> = (0..rounds.len()).map(|round| memo.as_ref().is_some_and(|memo| memo.recalls(sample_rate, recipe, &rounds, &keys[round], round))).collect();
    let work = total.div_ceil(BLOCK) * recalled.iter().filter(|&&hit| !hit).count();
    let mut worked = 0;
    progress.report(worked, work);
    let mut rendered: Vec<Rendered> = Vec::with_capacity(recipe.rows.len());
    let mut captures: Vec<Vec<Arc<Capture>>> = Vec::with_capacity(rounds.len());
    for (round, rows) in rounds.iter().enumerate() {
        let fresh: Vec<Arc<Capture>> = match memo.as_ref().filter(|_| recalled[round]) {
            Some(memo) => memo.captures[round].clone(),
            None => {
                let (mut audio, mut renderer) = Audio::open(sample_rate, BLOCK, Seer::Blind);
                renderer.patient();
                renderer.start_workers(helpers(), || None);
                audio.sync(project, recipe.solo, &[], &recipe.waves);
                audio.play(0.0);
                renderer.drain();
                for Rendered { host, index, vision } in &rendered {
                    audio.link.send(Message::Vision { host: *host, index: *index, vision: vision.clone(), twin: false });
                }
                renderer.drain();
                for &(host, index) in rows {
                    renderer.capture(host, index, total);
                }
                let (mut l, mut r) = (vec![0.0; BLOCK], vec![0.0; BLOCK]);
                let mut done = 0;
                while done < total {
                    if preempted() {
                        return None;
                    }
                    let n = BLOCK.min(total - done);
                    renderer.render(&mut l[..n], &mut r[..n]);
                    done += n;
                    worked += 1;
                    progress.report(worked, work);
                }
                rows.iter().map(|&(host, index)| Arc::new(renderer.take_capture(host, index).unwrap_or_else(|| Capture::new(0)))).collect()
            }
        };
        for (&(host, index), capture) in rows.iter().zip(&fresh) {
            let Some(Row::Effect(slot)) = project.rack(host).get(index) else {
                continue;
            };
            let mut pass = Capture::clone(capture);
            if let Host::Send(send) = host {
                pass.scale(recipe.scale[send]);
            }
            let upstream = match host {
                Host::Track(track) => timing.tracks.get(track).map(|track| &track.rack),
                Host::Send(send) => timing.returns.get(send),
                Host::Master => Some(&timing.master),
            };
            let upstream = upstream.and_then(|rack| rack.rows.get(index)).map_or(0, |row| row.upstream);
            let lanes = shaping_lanes(project.automation(host), index, slot.reverse_band());
            if !reverse(&mut pass, slot, &lanes, upstream, frames_per_beat, &clock, &mut preempted) {
                return None;
            }
            rendered.push(Rendered { host, index, vision: Box::new(pass.into_vision(frames_per_beat, sample_rate, upstream)) });
        }
        captures.push(fresh);
    }
    let frames: usize = captures.iter().flatten().map(|capture| capture.frames()).sum();
    *memo = (frames <= MEMO_FRAMES).then(|| Memo { sample_rate, solo: recipe.solo, waves: recipe.waves.clone(), rounds, keys, captures });
    Some(rendered)
}

fn shaping_lanes<'a>(automation: &'a Automation, row: usize, reverse_band: Option<&str>) -> Vec<&'a Curve> {
    let level = |curve: &Curve| curve.aim.mix_row().is_some() || matches!(&curve.aim, Aim::Knob { band, name, .. } if (reverse_band == Some(band.as_str()) && matches!(name.as_str(), "DRY" | "FORWARD" | "REVERSE")) || (band == "FOLLOW" && name == "DRY"));
    automation.curves.iter().filter(|curve| curve.aim.row_or_top() == row && curve.lfo.is_none() && !level(curve)).collect()
}

fn shaped(forward: &fx::Slot, lanes: &[&Curve], beat: f64) -> fx::Slot {
    let mut params = forward.params;
    for curve in lanes {
        if let Aim::Knob { band, name, .. } = &curve.aim {
            params.set_travel(band, name, curve.at(beat));
        }
    }
    fx::Slot { params, ..*forward }
}

fn reverse(capture: &mut Capture, slot: &fx::Slot, lanes: &[&Curve], upstream: usize, frames_per_beat: f64, clock: &Clock, preempted: &mut impl FnMut() -> bool) -> bool {
    let forward = slot.forward();
    let mut unit = Effect::new(slot.kind(), clock.sample_rate, PASS_BLOCK);
    let (mut silent_l, mut silent_r) = (vec![0.0; PASS_BLOCK], vec![0.0; PASS_BLOCK]);
    for _ in 0..SETTLE_BLOCKS {
        unit.process(&mut silent_l, &mut silent_r, &forward, clock);
        silent_l.fill(0.0);
        silent_r.fill(0.0);
    }
    capture.l.reverse();
    capture.r.reverse();
    let frames = capture.l.len();
    let step = if lanes.is_empty() { PASS_BLOCK } else { BLOCK };
    for start in (0..frames).step_by(step) {
        if preempted() {
            return false;
        }
        let end = (start + step).min(frames);
        let played = (!lanes.is_empty()).then(|| shaped(&forward, lanes, (frames - 1 - start).saturating_sub(upstream) as f64 / frames_per_beat));
        unit.process(&mut capture.l[start..end], &mut capture.r[start..end], played.as_ref().unwrap_or(&forward), clock);
    }
    capture.l.reverse();
    capture.r.reverse();
    true
}

#[derive(Default)]
pub struct Progress(AtomicU64);

impl Progress {
    fn report(&self, done: usize, work: usize) {
        self.0.store(((done as u64) << 32) | work as u64, Ordering::Relaxed);
    }

    fn rest(&self) {
        self.0.store(0, Ordering::Relaxed);
    }

    pub fn fraction(&self) -> Option<f32> {
        let word = self.0.load(Ordering::Relaxed);
        let (done, work) = ((word >> 32) as u32, word as u32);
        if work == 0 { None } else { Some(done as f32 / work as f32) }
    }
}

pub struct Job {
    generation: u64,
    sample_rate: f64,
    recipe: Option<Recipe>,
}

pub enum Seer {
    Blind,
    Inline,
    Thread { jobs: Sender<Job>, results: Receiver<(u64, Vec<Rendered>)>, generation: u64, progress: Arc<Progress> },
}

impl Seer {
    pub fn spawn() -> Seer {
        let (jobs, queue) = channel();
        let (sender, results) = channel();
        let progress = Arc::new(Progress::default());
        let served = Arc::clone(&progress);
        let _ = std::thread::Builder::new().name(String::from("bp-seer")).spawn(move || serve(&queue, &sender, &served));
        Seer::Thread { jobs, results, generation: 0, progress }
    }

    pub fn sees(&self) -> bool {
        !matches!(self, Seer::Blind)
    }

    pub(super) fn sibling(&self) -> Seer {
        match self {
            Seer::Blind => Seer::Blind,
            Seer::Inline => Seer::Inline,
            Seer::Thread { .. } => Seer::spawn(),
        }
    }

    pub fn dispatch(&mut self, recipe: Option<Recipe>, sample_rate: f64) -> Vec<Rendered> {
        match self {
            Seer::Blind => Vec::new(),
            Seer::Inline => recipe.and_then(|recipe| render(&recipe, sample_rate, &Progress::default(), &mut None, || false)).unwrap_or_default(),
            Seer::Thread { jobs, generation, .. } => {
                *generation += 1;
                let _ = jobs.send(Job { generation: *generation, sample_rate, recipe });
                Vec::new()
            }
        }
    }

    pub fn progress(&self) -> Option<f32> {
        let Seer::Thread { progress, .. } = self else {
            return None;
        };
        progress.fraction()
    }

    pub fn poll(&mut self) -> Vec<Rendered> {
        let Seer::Thread { results, generation, .. } = self else {
            return Vec::new();
        };
        let mut latest = None;
        while let Ok((rendered_for, rendered)) = results.try_recv() {
            if rendered_for == *generation {
                latest = Some(rendered);
            }
        }
        latest.unwrap_or_default()
    }
}

fn serve(queue: &Receiver<Job>, results: &Sender<(u64, Vec<Rendered>)>, progress: &Progress) {
    let mut memo = None;
    let mut next = queue.recv().ok();
    while let Some(mut job) = next.take() {
        while let Ok(newer) = queue.try_recv() {
            job = newer;
        }
        let mut preempting = None;
        let rendered = job.recipe.as_ref().and_then(|recipe| {
            render(recipe, job.sample_rate, progress, &mut memo, || match queue.try_recv() {
                Ok(newer) => {
                    preempting = Some(newer);
                    true
                }
                Err(TryRecvError::Empty) => false,
                Err(TryRecvError::Disconnected) => true,
            })
        });
        progress.rest();
        if let Some(rendered) = rendered
            && results.send((job.generation, rendered)).is_err()
        {
            return;
        }
        next = preempting.or_else(|| queue.recv().ok());
    }
}

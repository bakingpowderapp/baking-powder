use super::*;
use super::vision::{feeders, remove_levels};
use crate::project::Return;

pub const BLOCK: usize = 64;
const PART: usize = BLOCK / fx::Heard::PARTS;
const HEADROOM: f32 = 1.0 / 16.0;
const RELEASES: f64 = 8.0;

pub struct Trace {
    heard: Vec<fx::Heard>,
}

pub type Tracing = Job<Trace>;

pub fn blocks_per_beat(bpm: f64) -> f64 {
    frames_per_beat(EXPORT_SAMPLE_RATE, bpm) / BLOCK as f64
}

impl Trace {
    pub fn start(signal: &Project, waves: &[Arc<Wave>], end: i32) -> Option<Tracing> {
        let span = (0, end);
        let expected = Reach::of(signal.bpm, span, Tail::Cut)?.expected();
        let (project, waves) = (signal.clone(), waves.to_vec());
        Job::spawn("bp-duck", expected, move |counted, stopped| {
            let mut blocks = Vec::with_capacity(expected / BLOCK);
            let (mut block, mut filled) = (fx::Heard::default(), 0);
            let hear = |left: &[f32], right: &[f32]| {
                for (l, r) in left.iter().zip(right) {
                    let level = l.abs().max(r.abs()) / HEADROOM;
                    block.peak = block.peak.max(level);
                    block.power += level * level / BLOCK as f32;
                    block.levels_db[filled / PART] += (20.0 * level.log10()).max(fx::Heard::QUIET_DB) / PART as f32;
                    filled += 1;
                    if filled == BLOCK {
                        blocks.push(std::mem::take(&mut block));
                        filled = 0;
                    }
                }
            };
            render_span(&project, &waves, span, Tail::Cut, None, &[], counted, stopped, None, hear, |_, _| {}, |_| {})?;
            Some(Trace { heard: blocks })
        })
    }

    pub fn gains(&self, automated: &Automated) -> Vec<f32> {
        let rate = EXPORT_SAMPLE_RATE / BLOCK as f64;
        let sung = self.heard.len();
        let mut turned = automated.turned();
        let mut quiet = 0;
        let fed = (0..).map_while(|block| {
            let slot = turned(block);
            let fx::Params::Sidechain(params) = slot.params else {
                return None;
            };
            if block == sung {
                quiet = ((params.hold_ms + RELEASES * params.release_ms) * 0.001 * rate).ceil() as usize;
            }
            (block < sung + quiet).then(|| (self.heard.get(block).copied().unwrap_or(fx::Heard::QUIET), params, slot.wet))
        });
        let ducked = fx::duck(fed, rate);
        let ahead = automated.slot.params.latency(EXPORT_SAMPLE_RATE) as f64 / BLOCK as f64;
        let after = |block: usize| block.checked_sub(1).map_or(1.0, |ended| ducked.get(ended).or(ducked.last()).copied().unwrap_or(1.0));
        let passes = automated.slot.times as i32;
        let gains = (0..=ducked.len()).map(|block| {
            let at = block as f64 + ahead;
            let (whole, part) = (at.floor() as usize, at.fract() as f32);
            (after(whole) + (after(whole + 1) - after(whole)) * part).powi(passes)
        });
        gains.collect()
    }
}

#[derive(PartialEq)]
pub struct Automated {
    slot: fx::Slot,
    curves: Vec<Curve>,
    lfos: Vec<LaneLfo>,
    notes: Vec<SongNote>,
    bpm: f64,
}

fn turn(slot: &mut fx::Slot, aim: &Aim, travel: f64) {
    match aim {
        Aim::Wet(_) => slot.wet = travel,
        Aim::Knob { band, name, .. } => {
            slot.params.set_travel(band, name, travel);
        }
        Aim::Gain | Aim::Send(_) | Aim::Level(_) => {}
    }
}

impl Automated {
    pub fn restarts(project: &Project, host: Host, row: usize) -> bool {
        let aimed = |aim: &Aim| matches!(aim, Aim::Wet(at) | Aim::Knob { row: at, .. } if *at == row);
        project.automation(host).lfos.iter().any(|lfo| aimed(&lfo.aim) && lfo.restarting())
    }

    pub fn of(project: &Project, host: Host, row: usize, notes: Vec<SongNote>) -> Option<Automated> {
        let Row::Effect(slot @ fx::Slot { params: fx::Params::Sidechain(_), .. }) = *project.rack(host).get(row)? else {
            return None;
        };
        let lanes = project.automation(host);
        let aimed = |aim: &Aim| matches!(aim, Aim::Wet(at) | Aim::Knob { row: at, .. } if *at == row);
        let curves: Vec<Curve> = lanes.curves.iter().filter(|curve| aimed(&curve.aim)).cloned().collect();
        let mut lfos: Vec<LaneLfo> = lanes.lfos.iter().filter(|lfo| lfo.amount > 0.0 && aimed(&lfo.aim)).cloned().collect();
        for curve in curves.iter().filter(|curve| curve.lfo.is_some()) {
            if !lfos.iter().any(|lfo| lfo.aim == curve.aim) {
                lfos.push(lanes.lfos.iter().find(|lfo| lfo.aim == curve.aim).cloned().unwrap_or_else(|| LaneLfo::blank(curve.aim.clone())));
            }
        }
        let notes = if lfos.iter().any(LaneLfo::restarting) { notes } else { Vec::new() };
        Some(Automated { slot, curves, lfos, notes, bpm: project.bpm })
    }

    fn turned(&self) -> impl FnMut(usize) -> fx::Slot + '_ {
        let per_beat = blocks_per_beat(self.bpm);
        let seconds = BLOCK as f64 / EXPORT_SAMPLE_RATE;
        let mut lagged: Vec<Option<f64>> = vec![None; self.lfos.len()];
        move |block| {
            let beat = block as f64 / per_beat;
            let mut slot = self.slot;
            for curve in self.curves.iter().filter(|curve| curve.lfo.is_none()) {
                turn(&mut slot, &curve.aim, curve.at(beat));
            }
            for (own, lfo) in self.lfos.iter().enumerate() {
                if self.lfos[..own].iter().any(|other| other.aim == lfo.aim) {
                    continue;
                }
                let share = match &lfo.aim {
                    Aim::Knob { band, name, .. } => slot.params.lane_share(band, name),
                    _ => 1.0,
                };
                let swing: f64 = self.lfos[own..].iter().filter(|other| other.aim == lfo.aim).map(|other| other.swing(&self.curves, &self.notes, beat, self.bpm, share)).sum();
                let travel = (lfo.sweep(&self.curves, beat, share).centre() + swing).clamp(0.0, 1.0);
                let eased = lagged[own].map_or(travel, |held| held + (travel - held) * lfo.lag_alpha(seconds));
                lagged[own] = Some(eased);
                turn(&mut slot, &lfo.aim, eased);
            }
            slot
        }
    }
}

pub fn recipe(project: &Project, host: Host, source: usize, pre: bool) -> Project {
    let tracks = &project.tracks;
    let group = project.group_of(source);
    let sounds = source < tracks.len() && host != Host::Track(source);
    let mut needed = vec![false; tracks.len()];
    let mut queue = if sounds { vec![source] } else { Vec::new() };
    while let Some(track) = queue.pop() {
        if track < tracks.len() && !std::mem::replace(&mut needed[track], true) {
            queue.extend(feeders(project, track, tracks[track].rack.len()));
        }
    }
    let mut signal = Project::new();
    (signal.bpm, signal.root, signal.scale) = (project.bpm, project.root, project.scale);
    signal.master = Return { gain: HEADROOM, ..Return::DEFAULT };
    let kept = tracks.iter().enumerate().map(|(index, track)| {
        if !needed[index] {
            return Track { audio: track.audio, group: track.group, inside: track.inside, ..Track::audio() };
        }
        let mut kept = Track { name: String::new(), brief: String::new(), solo: index == source, mute: track.mute && index != source, sends: [0.0; SEND_COUNT], folded: false, ..track.clone() };
        remove_levels(&mut kept.automation, |aim| matches!(aim, Aim::Send(_)));
        kept
    });
    signal.tracks = kept.collect();
    if !sounds {
        return signal;
    }
    if let Some(group) = group {
        let shared = host.track().is_some_and(|track| track == group || project.group_of(track) == Some(group));
        signal.tracks[group] = Track { delay_ms: if shared { 0.0 } else { tracks[group].delay_ms }, ..Track::group() };
    }
    if pre {
        for row in (0..tracks[source].rack.len()).rev().filter(|&row| tracks[source].rack[row].is_effect() || tracks[source].rack[row].is_bus()) {
            signal.remove_row(Host::Track(source), row);
        }
        let tapped = &mut signal.tracks[source];
        tapped.gain = 1.0;
        remove_levels(&mut tapped.automation, |aim| *aim == Aim::Gain);
    }
    signal
}

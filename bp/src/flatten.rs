use crate::arp::MidiRow;
use fx::Kind;
use crate::project::{self, Aim, Automation, BEATS_PER_BAR, Clip, Host, LONGEST_TICKS, Layer, Note, Project, Row, SongNote, Track, copies, places};

pub struct Sent {
    tracks: Vec<Vec<Vec<SongNote>>>,
}

impl Sent {
    pub fn of(project: &Project, engine: Vec<Vec<Vec<SongNote>>>) -> Sent {
        let tracks = project
            .tracks
            .iter()
            .enumerate()
            .map(|(index, track)| {
                let places = places(&track.rack);
                let rows = engine.get(index);
                (0..track.rack.len()).map(|row| copies(&places, row).end.checked_sub(1).and_then(|at| rows.and_then(|rows| rows.get(at))).cloned().unwrap_or_default()).collect()
            })
            .collect();
        Sent { tracks }
    }

    pub fn notes(&self, track: usize, row: usize) -> &[SongNote] {
        self.tracks.get(track).and_then(|rows| rows.get(row)).map_or(&[], Vec::as_slice)
    }

    pub fn heard(&self, project: &Project, track: usize, aim: &Aim) -> Vec<SongNote> {
        let Some(owner) = project.tracks.get(track) else {
            return Vec::new();
        };
        let rack = &owner.rack;
        let source = match aim.row() {
            Some(row) => fed(rack, row),
            None => last_midi(rack),
        };
        match source {
            Some(row) => self.notes(track, row).to_vec(),
            None => {
                let mut notes = owner.song_notes();
                if let Some(leader) = owner.follows.filter(|&leader| leader != track) {
                    notes.extend(project.tracks.get(leader).and_then(|led| last_midi(&led.rack)).map_or(&[][..], |row| self.notes(leader, row)));
                    notes.sort_by(|a, b| a.start.total_cmp(&b.start));
                }
                notes
            }
        }
    }

    pub fn cored(&self, project: &Project, track: usize) -> Vec<SongNote> {
        let Some(owner) = project.tracks.get(track) else {
            return Vec::new();
        };
        let mut feeding: Vec<usize> = (0..owner.rack.len()).filter(|&at| owner.rack[at].is_core()).filter_map(|core| fed(&owner.rack, core)).collect();
        feeding.sort_unstable();
        feeding.dedup();
        let mut notes: Vec<SongNote> = feeding.iter().flat_map(|&row| self.notes(track, row).iter().copied()).collect();
        notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
        notes.dedup();
        notes
    }
}

fn fed(rack: &[Row], at: usize) -> Option<usize> {
    let mut chain = None;
    for (position, row) in rack.iter().enumerate().take(at) {
        match row {
            Row::Midi(_) => chain = Some(position),
            Row::Bus(_) => chain = chain.and(Some(0)),
            Row::Core(_) | Row::Effect(_) => {}
        }
    }
    chain
}

fn last_midi(rack: &[Row]) -> Option<usize> {
    rack.iter().rposition(Row::is_midi)
}

fn feeding(rack: &[Row], row: usize) -> impl Iterator<Item = usize> + '_ {
    (0..=row).filter(move |&at| at == row || project::reach(rack, at).contains(&row))
}

fn written(midi: MidiRow) -> MidiRow {
    let mut flat = midi.as_written();
    flat.arp.on = false;
    flat
}

pub fn spans<'a>(clips: impl Iterator<Item = &'a Clip>) -> Vec<Clip> {
    let mut spans: Vec<Clip> = clips.map(|clip| Clip { notes: Vec::new(), ..clip.clone() }).collect();
    spans.sort_by_key(|clip| clip.start);
    let mut merged: Vec<Clip> = Vec::with_capacity(spans.len());
    for clip in spans {
        match merged.last_mut() {
            Some(last) if clip.start < last.end() => last.length = last.end().max(clip.end()) - last.start,
            _ => merged.push(clip),
        }
    }
    merged
}

pub fn overdub(clips: &[Clip], played: &[SongNote]) -> Vec<Clip> {
    clips
        .iter()
        .enumerate()
        .map(|(index, clip)| {
            let room = clips.get(index + 1).map_or(i32::MAX, |next| next.start);
            let (start, end) = (project::beats(clip.start), project::beats(clip.end()));
            let inside: Vec<&SongNote> = played.iter().filter(|note| note.start >= start && note.start < end).collect();
            let tail = inside.iter().map(|note| project::ticks(note.end.ceil())).max().unwrap_or(clip.end());
            let mut joined = clip.clone();
            joined.length = tail.min(room).max(clip.end()) - clip.start;
            let edge = project::beats(joined.end());
            for note in inside {
                joined.place_note(Note { pitch: note.pitch, start: (note.start - start) as f32, length: (note.end.min(edge) - note.start) as f32, velocity: note.velocity, articulation: note.articulation });
            }
            joined
        })
        .collect()
}

fn bar_below(beat: f64) -> i32 {
    project::ticks((beat / f64::from(BEATS_PER_BAR)).floor() * f64::from(BEATS_PER_BAR))
}

fn bar_above(beat: f64) -> i32 {
    project::ticks((beat / f64::from(BEATS_PER_BAR)).ceil() * f64::from(BEATS_PER_BAR))
}

fn noted(note: &SongNote, from: f64) -> Note {
    let start = (note.start - from) as f32;
    let end = note.end - from;
    let near = (end - f64::from(start)) as f32;
    let length = [near.next_down(), near, near.next_up()].into_iter().filter(|length| *length > 0.0).min_by(|a, b| (f64::from(start + *a) - end).abs().total_cmp(&(f64::from(start + *b) - end).abs())).unwrap_or(near);
    Note { pitch: note.pitch, start, length, velocity: note.velocity, articulation: note.articulation }
}

pub fn print(template: &[Clip], notes: &[SongNote]) -> Vec<Clip> {
    let mut laid: Vec<Clip> = spans(template.iter());
    for note in notes {
        let holder = laid.iter().position(|clip| project::beats(clip.start) <= note.start && note.start < project::beats(clip.end()));
        let reach = project::ticks(note.end.ceil()).min(LONGEST_TICKS);
        match holder {
            Some(at) => laid[at].length = laid[at].length.max(reach - laid[at].start),
            None => {
                let start = bar_below(note.start);
                laid.push(Clip { start, length: bar_above(note.end).max(reach).min(LONGEST_TICKS) - start, label: String::new(), notes: Vec::new(), file: None });
            }
        }
        laid = spans(laid.iter());
    }
    for note in notes {
        if let Some(clip) = laid.iter_mut().find(|clip| project::beats(clip.start) <= note.start && note.start < project::beats(clip.end())) {
            let from = project::beats(clip.start);
            clip.place_note(noted(note, from));
        }
    }
    laid
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flattened {
    Nothing,
    Printed,
    Split { removed: u64, copied: bool, muted: bool },
}

const BYPASSED: &str = "Row is bypassed";
const ARP_OFF: &str = "Arp is off";
const FULL: &str = "Song is full";
const SHARED: &str = "Shared effects would change";
const EFFECT_LFO: &str = "Effect LFO would change";
const TRACK_LFO: &str = "Volume or send LFO would change";
const SIGNAL: &str = "Another row takes its signal";

fn chain_of(rack: &[Row], row: usize) -> (usize, Vec<usize>) {
    let reach = project::reach(rack, row).end;
    let chain = (row..reach).filter(|&at| at == row || (rack[at].takes_notes() && feeding(rack, at).any(|above| above == row))).collect();
    (reach, chain)
}

fn left_outside(rack: &[Row], chain: &[usize]) -> bool {
    rack.iter().enumerate().any(|(at, entry)| entry.is_core() && !chain.contains(&at)) || last_midi(rack).is_some_and(|last| !chain.contains(&last))
}

fn bends(entry: &Row) -> bool {
    matches!(entry, Row::Effect(slot) if !slot.bypass && slot.wet > 0.0 && !matches!(slot.kind(), Kind::Eq | Kind::Utility | Kind::Delay | Kind::Reverb))
}

fn restarting(automation: &Automation, row: Option<usize>) -> bool {
    automation.lfos.iter().any(|lfo| lfo.aim.row() == row && lfo.restarting() && (lfo.amount > 0.0 || automation.curves.iter().any(|curve| curve.lfo.is_some() && curve.aim == lfo.aim)))
}

fn signalled(project: &Project, index: usize) -> bool {
    let racks = project.tracks.iter().map(|track| &track.rack).chain(project.sends.iter().map(|send| &send.rack)).chain(std::iter::once(&project.master.rack));
    racks.flatten().any(|entry| matches!(entry, Row::Effect(slot) if slot.source().is_some_and(|(source, _)| source == index)))
}

pub fn refusal(project: &Project, index: usize, row: usize) -> Option<&'static str> {
    let track = project.tracks.get(index)?;
    let Some(Row::Midi(midi)) = track.rack.get(row) else {
        return None;
    };
    if midi.bypass {
        return Some(BYPASSED);
    }
    if !midi.arp.on {
        return Some(ARP_OFF);
    }
    let rack = &track.rack;
    let (reach, chain) = chain_of(rack, row);
    if !left_outside(rack, &chain) {
        return None;
    }
    if project.tracks.len() >= project::TRACKS {
        return Some(FULL);
    }
    let top = project::split(rack).filter(|&bus| bus < row).map_or(0, |bus| bus + 1);
    let shared = chain.iter().any(|&at| rack[at].is_core()) && rack[top..row].iter().any(Row::is_core);
    let copied: Vec<usize> = (row..reach).filter(|&at| rack[at].is_effect()).collect();
    if shared && copied.iter().any(|&at| bends(&rack[at])) {
        return Some(SHARED);
    }
    if shared && copied.iter().any(|&at| restarting(&track.automation, Some(at))) {
        return Some(EFFECT_LFO);
    }
    if restarting(&track.automation, None) {
        return Some(TRACK_LFO);
    }
    signalled(project, index).then_some(SIGNAL)
}

pub fn row(project: &mut Project, index: usize, row: usize, sent: &Sent) -> Flattened {
    let Some(track) = project.tracks.get(index) else {
        return Flattened::Nothing;
    };
    let Some(Row::Midi(midi)) = track.rack.get(row).copied() else {
        return Flattened::Nothing;
    };
    if refusal(project, index, row).is_some() {
        return Flattened::Nothing;
    }
    let printed = sent.notes(index, row).to_vec();
    let rack = track.rack.clone();
    let (reach, chain) = chain_of(&rack, row);
    let downstream = |at: usize| chain.contains(&at) || (row..reach).contains(&at) && rack[at].is_effect();
    let last = last_midi(&rack);
    let outside = left_outside(&rack, &chain);
    let fed = project.fed_by(index);
    let template = if fed == 0 { track.clips.clone() } else { spans(project.tracks.iter().enumerate().filter(|(source, _)| (fed | 1 << index) & (1 << source) != 0).flat_map(|(_, source)| &source.clips)) };
    let clips = print(&template, &printed);
    if !outside {
        let feeders: Vec<usize> = feeding(&rack, row).filter(|&at| rack[at].is_midi()).collect();
        let track = &mut project.tracks[index];
        for &at in &feeders {
            if let Row::Midi(midi) = &mut track.rack[at] {
                *midi = written(*midi);
                if at < row {
                    midi.force_key = false;
                }
            }
        }
        let baked = |aimed: Option<usize>| aimed.is_some_and(|aimed| feeders.contains(&aimed));
        track.automation.curves.retain(|curve| !baked(curve.aim.row()));
        track.automation.lfos.retain(|lfo| !baked(lfo.aim.row()));
        track.clips = clips;
        track.follows = None;
        return Flattened::Printed;
    }
    let cored = chain.iter().any(|&at| rack[at].is_core());
    let moved: Vec<usize> = (row..reach).filter(|&at| downstream(at) && (cored || !rack[at].is_effect())).collect();
    let landing = |at: usize| moved.iter().position(|&kept| kept == at);
    let mut split_rack: Vec<Row> = moved.iter().map(|&at| if at == row { Row::Midi(written(midi)) } else { rack[at] }).collect();
    if !cored {
        split_rack.push(Row::Core(Layer::DEFAULT));
    }
    project::set_level(&mut split_rack, 0, project::level(&rack, usize::from(project::split(&rack).is_some_and(|at| at < row))));
    let track = &project.tracks[index];
    let lfos = track.automation.lfos.iter().filter(|lfo| lfo.aim.row() != Some(row)).cloned().filter_map(|mut lfo| lfo.aim.remap(&landing).then_some(lfo)).collect();
    let curves = track.automation.curves.iter().filter(|curve| curve.aim.row() != Some(row)).cloned().filter_map(|mut curve| curve.aim.remap(&landing).then_some(curve)).collect();
    let copied = cored && moved.iter().any(|&at| rack[at].is_effect());
    let split = Track { name: String::new(), brief: String::new(), sound: track.sound.clone(), sound_desc: track.sound_desc.clone(), solo: track.solo, mute: track.mute || !cored, gain: track.gain, delay_ms: track.delay_ms, sends: track.sends, rack: split_rack, clips, automation: Automation { curves, lfos }, audio: false, group: false, inside: false, folded: false, follows: None };
    let carries = last.is_some_and(|last| chain.contains(&last));
    project.add_track(index + 1, split);
    project.settle_signals(Host::Track(index + 1));
    if carries {
        for follower in &mut project.tracks {
            if follower.follows == Some(index) {
                follower.follows = Some(index + 1);
            }
        }
    }
    let removed: Vec<usize> = chain.iter().copied().filter(|&at| !rack[at].is_effect()).collect();
    for &at in removed.iter().rev() {
        project.remove_row(Host::Track(index), at);
    }
    project::tidy(&mut project.tracks[index].rack);
    Flattened::Split { removed: removed.iter().fold(0, |bits, &at| bits | 1 << at), copied, muted: !cored }
}

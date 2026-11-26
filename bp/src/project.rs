use crate::arp::MidiRow;
use crate::theory::{self, Root, ScaleName};

mod automation;
mod clips;
pub mod codec;
mod harmony;
mod landing;
pub mod manual;
mod rows;
mod times;
mod timing;
pub mod warp;

pub use automation::*;
pub use clips::*;
pub use harmony::*;
pub use landing::*;
pub use rows::*;
pub use times::*;
pub use timing::*;

pub const BEATS_PER_BAR: i32 = 4;
const TOP_PITCH: i32 = 127;
pub const TICKS_PER_BEAT: i32 = 960;
pub const TICKS_PER_BAR: i32 = TICKS_PER_BEAT * BEATS_PER_BAR;
pub const LONGEST_TICKS: i32 = (1 << 20) * TICKS_PER_BEAT;
pub const VELOCITY: u8 = 127;
pub const DEFAULT_BPM: f64 = 120.0;
pub const LEAST_BPM: f64 = 20.0;
pub const MOST_BPM: f64 = 300.0;
pub const TRACKS: usize = 64;
pub const SEND_COUNT: usize = 16;
pub const RACK_ROWS: usize = 40;
pub const SEEDS: u64 = (TRACKS * RACK_ROWS) as u64;
pub const MIX: &str = "MIX";
pub const VOL: &str = "VOL";
pub const GAIN: &str = "GAIN";
pub const SEND: &str = "SEND";
pub const SEND_NAMES: [&str; SEND_COUNT] = ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P"];

pub const UNITY_POSITION: f32 = 0.75;
pub const TOP_DB: f32 = 6.0;
pub const DELAY_MS: f32 = 100.0;

pub fn gain_to_position(gain: f32) -> f32 {
    if gain >= 1.0 {
        UNITY_POSITION + 20.0 * gain.log10() / TOP_DB * (1.0 - UNITY_POSITION)
    } else {
        gain * UNITY_POSITION
    }
}

pub fn position_to_gain(position: f32) -> f32 {
    if position >= UNITY_POSITION {
        10f32.powf((position - UNITY_POSITION) / (1.0 - UNITY_POSITION) * TOP_DB / 20.0)
    } else {
        position / UNITY_POSITION
    }
}

pub fn beats(ticks: i32) -> f64 {
    f64::from(ticks) / f64::from(TICKS_PER_BEAT)
}

pub fn ticks(beats: f64) -> i32 {
    (beats * f64::from(TICKS_PER_BEAT)).round() as i32
}

#[derive(Clone, PartialEq, Debug)]
pub struct Track {
    pub name: String,
    pub brief: String,
    pub sound: String,
    pub sound_desc: String,
    pub solo: bool,
    pub mute: bool,
    pub gain: f32,
    pub delay_ms: f32,
    pub sends: [f32; SEND_COUNT],
    pub rack: Vec<Row>,
    pub clips: Vec<Clip>,
    pub automation: Automation,
    pub audio: bool,
    pub group: bool,
    pub inside: bool,
    pub folded: bool,
    pub follows: Option<usize>,
}

impl Track {
    pub fn renumber(&mut self, map: impl Fn(usize) -> Option<usize>) {
        self.follows = self.follows.and_then(&map);
        for row in &mut self.rack {
            if let Row::Effect(slot) = row {
                slot.renumber(&map);
            }
        }
    }

    pub fn blank() -> Track {
        Track { name: String::new(), brief: String::new(), sound: String::new(), sound_desc: String::new(), solo: false, mute: false, gain: 1.0, delay_ms: 0.0, sends: [0.0; SEND_COUNT], rack: vec![Row::Midi(MidiRow::DEFAULT), Row::Core(Layer::DEFAULT)], clips: Vec::new(), automation: Automation::NONE, audio: false, group: false, inside: false, folded: false, follows: None }
    }

    pub fn audio() -> Track {
        Track { rack: Vec::new(), audio: true, ..Track::blank() }
    }

    pub fn group() -> Track {
        Track { rack: Vec::new(), group: true, ..Track::blank() }
    }

    pub fn kind(&self) -> &'static str {
        if self.group {
            "GROUP"
        } else if self.audio {
            "AUDIO"
        } else {
            "MIDI"
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Project {
    pub brief: String,
    pub root: Root,
    pub scale: ScaleName,
    pub decks: [[PaletteColumn; 7]; DECKS],
    pub tracks: Vec<Track>,
    pub bpm: f64,
    pub master: Return,
    pub sends: [Return; SEND_COUNT],
    pub metronome: bool,
    pub lock: bool,
    next_number: u32,
}

impl Default for Project {
    fn default() -> Project {
        Project::new()
    }
}

impl Project {
    pub fn end(&self) -> i32 {
        self.tracks.iter().flat_map(|track| &track.clips).map(Clip::end).max().unwrap_or(0)
    }

    pub fn key_bits(&self) -> u16 {
        theory::key_bits(self.root, self.scale)
    }

    pub fn transposed(&mut self, semitones: i32) -> bool {
        let leaves = |note: &Note| !(0..=TOP_PITCH).contains(&(i32::from(note.pitch) + semitones));
        if semitones == 0 || self.tracks.iter().flat_map(|track| &track.clips).flat_map(|clip| &clip.notes).any(leaves) {
            return false;
        }
        for note in self.tracks.iter_mut().flat_map(|track| &mut track.clips).flat_map(|clip| &mut clip.notes) {
            note.pitch = (i32::from(note.pitch) + semitones) as u8;
        }
        self.root = Root::ALL[(self.root.index() as i32 + semitones).rem_euclid(Root::ALL.len() as i32) as usize];
        true
    }

    pub fn match_sidechains(&mut self, host: Host, row: usize) -> Option<Vec<(Host, usize)>> {
        let Some(Row::Effect(fx::Slot { params: fx::Params::Sidechain(like), .. })) = self.rack(host).get(row).copied() else {
            return None;
        };
        let hosts: Vec<Host> = (0..self.tracks.len()).map(Host::Track).chain((0..SEND_COUNT).map(Host::Send)).chain(std::iter::once(Host::Master)).collect();
        let mut matched = Vec::new();
        for owner in hosts {
            for (index, entry) in self.rack_mut(owner).iter_mut().enumerate() {
                if let Row::Effect(fx::Slot { params: fx::Params::Sidechain(held), .. }) = entry
                    && held.signal.source == like.signal.source
                    && (owner, index) != (host, row)
                {
                    *held = held.matched(&like);
                    matched.push((owner, index));
                }
            }
        }
        Some(matched)
    }

    pub fn insert_time(&mut self, at: i32, by: i32) {
        let bpm = self.bpm;
        for track in &mut self.tracks {
            track.insert(at, by, bpm);
        }
        for ret in self.returns_mut() {
            ret.automation.open(at, by);
        }
    }

    pub fn returns(&self) -> impl Iterator<Item = &Return> {
        self.sends.iter().chain(std::iter::once(&self.master))
    }

    pub fn returns_mut(&mut self) -> impl Iterator<Item = &mut Return> {
        self.sends.iter_mut().chain(std::iter::once(&mut self.master))
    }

    pub fn automation(&self, host: Host) -> &Automation {
        match host {
            Host::Track(track) => &self.tracks[track].automation,
            Host::Send(send) => &self.sends[send].automation,
            Host::Master => &self.master.automation,
        }
    }

    pub fn automation_mut(&mut self, host: Host) -> &mut Automation {
        match host {
            Host::Track(track) => &mut self.tracks[track].automation,
            Host::Send(send) => &mut self.sends[send].automation,
            Host::Master => &mut self.master.automation,
        }
    }

    pub fn gain_of(&self, host: Host) -> f32 {
        match host {
            Host::Track(track) => self.tracks[track].gain,
            Host::Send(send) => self.sends[send].gain,
            Host::Master => self.master.gain,
        }
    }

    pub fn song_notes_of(&self, host: Host) -> Vec<SongNote> {
        match host {
            Host::Track(track) => self.fed_notes(track),
            Host::Send(_) | Host::Master => Vec::new(),
        }
    }

    pub fn beside(&self, track: usize, clip: &Clip) -> Vec<Note> {
        let (start, end) = (clip.start, clip.end());
        let mut notes = Vec::new();
        for (index, other) in self.tracks.iter().enumerate() {
            for near in other.clips.iter().filter(|near| (index, near.start) != (track, start) && near.start < end && near.end() > start) {
                let offset = beats(near.start - start) as f32;
                for note in &near.notes {
                    let from = (note.start + offset).max(0.0);
                    let to = ((note.start + note.length).min(near.beats()) + offset).min(clip.beats());
                    if to > from {
                        notes.push(Note { start: from, length: to - from, ..*note });
                    }
                }
            }
        }
        notes
    }

    fn blank() -> Project {
        Project { brief: String::new(), root: Root::C, scale: ScaleName::Major, decks: [[PaletteColumn::DEFAULT; 7]; DECKS], tracks: Vec::new(), bpm: DEFAULT_BPM, master: Return::DEFAULT, sends: [Return::DEFAULT; SEND_COUNT], metronome: false, lock: false, next_number: 1 }
    }

    pub fn new() -> Project {
        let mut project = Project::blank();
        project.add_midi_track(0);
        project.add_midi_track(1);
        project.seed();
        project
    }

    pub fn seed(&mut self) {
        let (mut taken, mut unset) = (Vec::new(), Vec::new());
        for (track, owner) in self.tracks.iter_mut().enumerate() {
            for (row, entry) in owner.rack.iter_mut().enumerate() {
                if let Row::Core(layer) = entry {
                    if layer.seed == 0 || taken.contains(&layer.seed) {
                        unset.push((track, row));
                    } else {
                        taken.push(layer.seed);
                    }
                }
            }
        }
        for (track, row) in unset {
            let place = (track * RACK_ROWS + row) as u64;
            let seed = if place == 0 || taken.contains(&place) { (SEEDS..).find(|seed| !taken.contains(seed)).unwrap_or(SEEDS) } else { place };
            taken.push(seed);
            if let Row::Core(layer) = &mut self.tracks[track].rack[row] {
                layer.seed = seed;
            }
        }
    }

    pub fn unloop_gates(&mut self) {
        for (index, track) in self.tracks.iter_mut().enumerate() {
            for row in &mut track.rack {
                *row = row.unsignalled_gate(|source| source == index);
            }
        }
    }

    pub fn add_midi_track(&mut self, at: usize) -> bool {
        self.add_track(at, Track::blank())
    }

    pub fn add_track(&mut self, at: usize, track: Track) -> bool {
        let inside = !track.group && self.joins_at(at);
        self.place_track(at, track, inside)
    }

    pub fn add_track_outside(&mut self, at: usize, track: Track) -> bool {
        self.place_track(at, track, false)
    }

    fn place_track(&mut self, at: usize, track: Track, inside: bool) -> bool {
        if self.tracks.len() >= TRACKS {
            return false;
        }
        self.seat_track(at, track, inside);
        self.renumber(|source| Some(inserted_index(source, at, 1)));
        true
    }

    fn seat_track(&mut self, at: usize, mut track: Track, inside: bool) {
        if track.name.is_empty() {
            track.name = format!("{} {}", track.kind(), self.next_number);
            self.next_number += 1;
        }
        track.inside = inside;
        self.tracks.insert(at, track);
    }

    pub fn insert_tracks(&mut self, at: usize, tracks: Vec<Track>) -> usize {
        let standing = self.tracks.len();
        self.insert_renumbered(at, tracks, |source, added| (source < standing).then_some(inserted_index(source, at, added)))
    }

    pub fn insert_foreign_tracks(&mut self, at: usize, tracks: Vec<Track>, homes: &[Option<usize>]) -> usize {
        self.insert_renumbered(at, tracks, |source, added| homes.iter().position(|&home| home == Some(source)).filter(|&copy| copy < added).map(|copy| at + copy))
    }

    fn insert_renumbered(&mut self, at: usize, tracks: Vec<Track>, renumbered: impl Fn(usize, usize) -> Option<usize>) -> usize {
        let added = tracks.len().min(TRACKS.saturating_sub(self.tracks.len()));
        self.renumber(|source| Some(inserted_index(source, at, added)));
        let mut grouped = false;
        for (copy, mut track) in tracks.into_iter().take(added).enumerate() {
            track.rack.iter_mut().for_each(|row| *row = row.unseeded());
            track.renumber(|source| renumbered(source, added));
            let inside = if grouped { track.inside } else { !track.group && self.joins_at(at + copy) };
            grouped |= track.group;
            self.seat_track(at + copy, track, inside);
        }
        for copy in at..at + added {
            self.settle_signals(Host::Track(copy));
        }
        added
    }

    pub fn signalled_on(&self, host: Host, row: Row) -> Row {
        row.renumbered(|source| self.can_signal(source, host).then_some(source))
    }

    pub fn settle_signals(&mut self, host: Host) {
        for index in 0..self.rack(host).len() {
            let kept = self.signalled_on(host, self.rack(host)[index]);
            self.rack_mut(host)[index] = kept;
        }
    }

    fn can_signal(&self, source: usize, host: Host) -> bool {
        source < self.tracks.len() && host.track().is_none_or(|track| source != track && !self.leans_on(source, track))
    }

    fn leans_on(&self, track: usize, on: usize) -> bool {
        let count = self.tracks.len().min(TRACKS);
        let takes = |index: usize| {
            let signals = self.tracks[index].rack.iter().fold(0, |signals, row| match row {
                Row::Effect(slot) => signals | source_of(slot, index, count),
                Row::Midi(_) | Row::Core(_) | Row::Bus(_) => signals,
            });
            self.members_of(index).into_iter().fold(signals, |taken, member| taken | (1 << member))
        };
        let (mut reached, mut waiting) = (0u64, 1u64 << track);
        while waiting != 0 {
            let next = waiting.trailing_zeros() as usize;
            waiting &= waiting - 1;
            let fresh = takes(next) & !reached;
            reached |= fresh;
            waiting |= fresh;
        }
        reached & (1 << on) != 0
    }

    pub fn copies(&self, chosen: &[usize]) -> Vec<Track> {
        chosen.iter().map(|&track| Track { inside: self.group_of(track).is_some_and(|group| chosen.contains(&group)), ..self.tracks[track].clone() }).collect()
    }

    pub fn duplicate_tracks(&mut self, chosen: &[usize]) -> usize {
        let copies = self.copies(chosen);
        chosen.last().map_or(0, |last| self.insert_tracks(last + 1, copies))
    }

    fn joins_at(&self, at: usize) -> bool {
        at > 0 && self.tracks.get(at - 1).is_some_and(|above| above.group || above.inside)
    }

    pub fn group_of(&self, track: usize) -> Option<usize> {
        if !self.tracks.get(track)?.inside {
            return None;
        }
        let above = (0..track).rev().find(|&index| !self.tracks[index].inside)?;
        self.tracks[above].group.then_some(above)
    }

    pub fn members_of(&self, group: usize) -> Vec<usize> {
        if !self.tracks.get(group).is_some_and(|track| track.group) {
            return Vec::new();
        }
        (group + 1..self.tracks.len()).take_while(|&track| self.tracks[track].inside).collect()
    }

    pub fn fed_by(&self, track: usize) -> u64 {
        let mut seen = 1u64 << track;
        let mut at = track;
        while let Some(source) = self.tracks.get(at).and_then(|follower| follower.follows).filter(|&source| seen & (1 << source) == 0) {
            seen |= 1 << source;
            at = source;
        }
        seen & !(1 << track)
    }

    pub fn fed_notes(&self, track: usize) -> Vec<SongNote> {
        let fed = self.fed_by(track);
        if fed == 0 {
            return self.tracks.get(track).map(Track::song_notes).unwrap_or_default();
        }
        let fed = fed | 1 << track;
        let mut notes: Vec<SongNote> = self.tracks.iter().enumerate().filter(|&(source, _)| fed & (1 << source) != 0).flat_map(|(_, source)| source.placed()).collect();
        notes.sort_by(|a, b| a.start.total_cmp(&b.start));
        notes
    }

    pub fn leading(&self, tracks: Vec<usize>) -> Vec<usize> {
        let mut leading: Vec<usize> = tracks.iter().copied().filter(|&track| !tracks.iter().any(|&other| other != track && self.fed_by(track) & (1 << other) != 0)).collect();
        leading.sort_unstable();
        leading.dedup();
        leading
    }

    pub fn follow_sources(&self, track: usize) -> Vec<usize> {
        if !self.tracks.get(track).is_some_and(Track::generates) {
            return Vec::new();
        }
        (0..self.tracks.len()).filter(|&source| source != track && self.tracks[source].generates()).collect()
    }

    pub fn move_track(&mut self, from: usize, to: usize) {
        for member in self.members_of(from) {
            self.tracks[member].inside = false;
        }
        let track = self.tracks.remove(from);
        self.tracks.insert(to, track);
        if !self.tracks[to].group {
            self.tracks[to].inside = self.joins_at(to);
        }
        self.renumber(|source| Some(moved_index(source, from, to)));
    }

    pub fn remove_track(&mut self, index: usize) -> bool {
        if self.tracks.len() <= 1 {
            return false;
        }
        for member in self.members_of(index) {
            self.tracks[member].inside = false;
        }
        self.tracks.remove(index);
        self.renumber(|source| if source == index { None } else { Some(source - usize::from(source > index)) });
        true
    }

    fn renumber(&mut self, map: impl Fn(usize) -> Option<usize>) {
        for track in &mut self.tracks {
            track.renumber(&map);
        }
        let racks = self.sends.iter_mut().map(|send| &mut send.rack).chain(std::iter::once(&mut self.master.rack));
        for row in racks.flatten() {
            if let Row::Effect(slot) = row {
                slot.renumber(&map);
            }
        }
    }
}

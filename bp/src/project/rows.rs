use super::{Aim, Automation, LaneLfo, PaletteColumn, Project, RACK_ROWS, Track, moved_lfos, room, taken};
use crate::arp::{Arp, MidiRow, Ties};
use coresynth::Params;
use fx::Slot;
use sdk::face::Blank;
use sdk::param::Knob;
use sdk::spectrum::Target;

const MASTER_BIT: u64 = 1 << 62;
const OUTPUT_BIT: u64 = 1 << 61;
const SEND_BIT: u64 = 1 << 60;

pub fn moved_index(index: usize, from: usize, to: usize) -> usize {
    if index == from {
        to
    } else if from < index && index <= to {
        index - 1
    } else if to <= index && index < from {
        index + 1
    } else {
        index
    }
}

pub fn inserted_index(index: usize, at: usize, count: usize) -> usize {
    if index >= at { index + count } else { index }
}

pub fn removed_index(index: usize, gone: usize) -> Option<usize> {
    match index.cmp(&gone) {
        std::cmp::Ordering::Less => Some(index),
        std::cmp::Ordering::Equal => None,
        std::cmp::Ordering::Greater => Some(index - 1),
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Layer {
    pub level: f64,
    pub patch: Params,
    pub seed: u64,
    pub times: usize,
}

impl Layer {
    pub const DEFAULT: Layer = Layer { level: 1.0, patch: Params::DEFAULT, seed: 0, times: 1 };
}

pub const BUSES: usize = 2;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Bus {
    pub levels: [f64; BUSES],
    pub master: usize,
    pub face: Blank,
}

impl Bus {
    pub const KIND: &'static str = "bus";
    pub const DEFAULT: Bus = Bus { levels: [1.0; BUSES], master: 0, face: Blank };
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Row {
    Midi(MidiRow),
    Core(Layer),
    Effect(Slot),
    Bus(Bus),
}

impl Row {
    pub fn unseeded(self) -> Row {
        match self {
            Row::Core(layer) => Row::Core(Layer { seed: 0, ..layer }),
            row => row,
        }
    }

    pub fn renumbered(mut self, map: impl Fn(usize) -> Option<usize>) -> Row {
        if let Row::Effect(slot) = &mut self {
            slot.renumber(map);
        }
        self
    }

    pub fn unsignalled_gate(self, from: impl Fn(usize) -> bool) -> Row {
        match self {
            Row::Effect(mut slot) => {
                slot.unsignal_gate(from);
                Row::Effect(slot)
            }
            row => row,
        }
    }

    pub fn is_core(&self) -> bool {
        matches!(self, Row::Core(_))
    }

    pub fn is_midi(&self) -> bool {
        matches!(self, Row::Midi(_))
    }

    pub fn is_effect(&self) -> bool {
        matches!(self, Row::Effect(_))
    }

    pub fn is_bus(&self) -> bool {
        matches!(self, Row::Bus(_))
    }

    pub fn takes_notes(&self) -> bool {
        !self.is_effect()
    }

    pub fn mixes(&self) -> bool {
        self.is_core() || self.is_effect()
    }

    pub fn same_kind(&self, other: &Row) -> bool {
        match (self, other) {
            (Row::Midi(_), Row::Midi(_)) | (Row::Core(_), Row::Core(_)) | (Row::Bus(_), Row::Bus(_)) => true,
            (Row::Effect(mine), Row::Effect(theirs)) => mine.kind() == theirs.kind(),
            _ => false,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Row::Midi(_) => MidiRow::LABEL,
            Row::Core(_) => "Core",
            Row::Effect(slot) => slot.kind().label(),
            Row::Bus(_) => "Bus",
        }
    }

    pub fn bypassed(&self) -> bool {
        match self {
            Row::Midi(midi) => midi.bypass,
            Row::Core(_) | Row::Bus(_) => false,
            Row::Effect(slot) => slot.bypass,
        }
    }

    pub fn set_bypass(&mut self, bypass: bool) {
        match self {
            Row::Midi(midi) => midi.bypass = bypass,
            Row::Core(_) | Row::Bus(_) => {}
            Row::Effect(slot) => slot.bypass = bypass,
        }
    }

    pub fn mix(&self) -> f64 {
        match self {
            Row::Midi(_) | Row::Bus(_) => 1.0,
            Row::Core(layer) => layer.level,
            Row::Effect(slot) => slot.wet,
        }
    }

    pub fn set_mix(&mut self, mix: f64) {
        match self {
            Row::Midi(_) | Row::Bus(_) => {}
            Row::Core(layer) => layer.level = mix,
            Row::Effect(slot) => slot.wet = mix,
        }
    }

    pub fn mix_aim(&self, at: usize) -> Option<Aim> {
        match self {
            Row::Midi(_) | Row::Bus(_) => None,
            Row::Core(_) => Some(Aim::Level(at)),
            Row::Effect(_) => Some(Aim::Wet(at)),
        }
    }

    pub fn mix_key(&self) -> &'static str {
        match self {
            Row::Midi(_) | Row::Bus(_) => "",
            Row::Core(_) => "level",
            Row::Effect(_) => "wet",
        }
    }

    pub fn knobs(&self) -> &'static [Knob] {
        match self {
            Row::Midi(_) => MidiRow::knobs(),
            Row::Core(_) => coresynth::params::knobs(),
            Row::Effect(slot) => slot.kind().knobs(),
            Row::Bus(_) => &[],
        }
    }

    pub fn mix_desc(&self) -> &'static str {
        match self {
            Row::Midi(_) | Row::Bus(_) => "",
            Row::Core(_) => "The Core's level into the rack",
            Row::Effect(_) => "The effect's blend, dry to wet",
        }
    }

    pub fn automatable(&self, band: &str, param: &str) -> bool {
        match self {
            Row::Midi(_) => MidiRow::automatable(band, param),
            Row::Core(_) => Params::automatable(band, param),
            Row::Effect(slot) => slot.params.automatable(band, param),
            Row::Bus(_) => false,
        }
    }

    pub fn travel_of(&self, band: &str, param: &str) -> Option<f64> {
        match self {
            Row::Midi(midi) => midi.travel_of(band, param),
            Row::Core(layer) => layer.patch.travel_of(band, param),
            Row::Effect(slot) => slot.params.travel_of(band, param),
            Row::Bus(_) => None,
        }
    }

    pub fn lane_share(&self, aim: &Aim) -> f64 {
        let Aim::Knob { band, name, .. } = aim else {
            return 1.0;
        };
        match self {
            Row::Midi(_) => MidiRow::lane_share(band, name),
            Row::Core(_) => Params::lane_share(band, name),
            Row::Effect(slot) => slot.params.lane_share(band, name),
            Row::Bus(_) => 1.0,
        }
    }

    pub fn set_travel(&mut self, band: &str, param: &str, travel: f64) -> bool {
        match self {
            Row::Midi(midi) => midi.set_travel(band, param, travel),
            Row::Core(layer) => layer.patch.set_travel(band, param, travel),
            Row::Effect(slot) => slot.params.set_travel(band, param, travel),
            Row::Bus(_) => false,
        }
    }

    pub fn latency(&self, sample_rate: f64) -> usize {
        match self {
            Row::Midi(_) | Row::Core(_) | Row::Bus(_) => 0,
            Row::Effect(slot) => slot.latency(sample_rate) * slot.times,
        }
    }
}

pub fn split(rack: &[Row]) -> Option<usize> {
    rack.iter().position(Row::is_bus)
}

pub fn level(rack: &[Row], bus: usize) -> f64 {
    rack.iter().find_map(|row| match row {
        Row::Bus(held) => held.levels.get(bus).copied(),
        _ => None,
    }).unwrap_or(1.0)
}

pub fn tidy(rack: &mut Vec<Row>) {
    if rack.last() == Some(&Row::Bus(Bus::DEFAULT)) {
        rack.pop();
    }
}

pub fn set_level(rack: &mut Vec<Row>, bus: usize, level: f64) -> bool {
    if split(rack).is_none() && room(rack) > 0 {
        rack.push(Row::Bus(Bus::DEFAULT));
    }
    let changed = rack.iter_mut().any(|row| match row {
        Row::Bus(held) if bus < BUSES && held.levels[bus] != level => {
            held.levels[bus] = level;
            true
        }
        _ => false,
    });
    tidy(rack);
    changed
}

pub fn shown(rack: &[Row], bus: usize) -> std::ops::Range<usize> {
    let (first, end) = (usize::from(rack.first().is_some_and(Row::is_midi)), rack.len());
    match (split(rack), bus) {
        (Some(at), 0) => first..at,
        (Some(at), _) => at + 1..end,
        (None, 0) => first..end,
        (None, _) => end..end,
    }
}

pub fn reach(rack: &[Row], row: usize) -> std::ops::Range<usize> {
    row + 1..split(rack).filter(|&at| 0 < row && row < at).unwrap_or(rack.len())
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Host {
    Track(usize),
    Send(usize),
    Master,
}

impl Host {
    pub fn track(self) -> Option<usize> {
        match self {
            Host::Track(track) => Some(track),
            Host::Send(_) | Host::Master => None,
        }
    }

    fn key(self) -> u64 {
        match self {
            Host::Track(track) => (track as u64) << 32,
            Host::Send(send) => SEND_BIT | (send as u64) << 32,
            Host::Master => MASTER_BIT,
        }
    }

    pub fn insert(self, row: usize) -> Target {
        Target::keyed(self.key() | row as u64)
    }

    pub fn output(self) -> Target {
        Target::keyed(OUTPUT_BIT | self.key())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cores {
    Required,
    Refused,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Return {
    pub gain: f32,
    pub rack: Vec<Row>,
    pub automation: Automation,
}

impl Return {
    pub const DEFAULT: Return = Return { gain: 1.0, rack: Vec::new(), automation: Automation::NONE };
}

fn kept_in(cores: Cores, row: &Row) -> bool {
    cores != Cores::Refused || !row.takes_notes()
}

fn kept(cores: Cores, rows: &[Row]) -> Vec<bool> {
    let mut split = false;
    let mut kept: Vec<bool> = rows.iter().map(|row| kept_in(cores, row) && !(row.is_bus() && std::mem::replace(&mut split, true))).collect();
    if let Some(last) = kept.iter().rposition(|kept| *kept)
        && rows[last] == Row::Bus(Bus::DEFAULT)
    {
        kept[last] = false;
    }
    kept
}

fn held(cores: Cores, rows: &[Row]) -> impl Iterator<Item = Row> + '_ {
    rows.iter().zip(kept(cores, rows)).filter(|(_, kept)| *kept).map(|(row, _)| *row)
}

fn settled(cores: Cores, rows: &[Row]) -> Vec<Row> {
    held(cores, rows).take(RACK_ROWS).collect()
}

pub fn wanted(cores: Cores, rows: &[Row]) -> usize {
    let held: Vec<Row> = held(cores, rows).collect();
    let added = usize::from(midiless(&held)) + usize::from(coreless(&held));
    taken(&held) + if cores == Cores::Required { added } else { 0 }
}

fn midiless(rack: &[Row]) -> bool {
    !rack.first().is_some_and(Row::is_midi)
}

fn coreless(rack: &[Row]) -> bool {
    !rack.iter().any(Row::is_core)
}

pub fn struck_at(rack: &[Row]) -> Option<u8> {
    rack.iter().find_map(|row| match row {
        Row::Midi(midi) => Some(midi.velocity),
        _ => None,
    })
}

pub fn transposed_at(rack: &[Row]) -> Option<i8> {
    match rack.first() {
        Some(Row::Midi(midi)) => Some(midi.transpose),
        _ => None,
    }
}

pub fn locked_arp(rack: &[Row]) -> Option<Arp> {
    rack.iter().find_map(|row| match row {
        Row::Midi(midi) if midi.arp.lock => Some(midi.arp),
        _ => None,
    })
}

pub fn forced_ties(rack: &[Row]) -> Option<Ties> {
    rack.iter().find_map(|row| match row {
        Row::Midi(midi) if midi.force_key => Some(midi.ties),
        _ => None,
    })
}

pub fn settled_rack(cores: Cores, rows: Vec<Row>) -> Vec<Row> {
    let mut rack = settled(cores, &rows);
    if cores == Cores::Required {
        if midiless(&rack) {
            rack.truncate(RACK_ROWS - 1);
            rack.insert(0, Row::Midi(MidiRow::DEFAULT));
        }
        if coreless(&rack) {
            rack.truncate(RACK_ROWS - 1);
            rack.insert(1, Row::Core(Layer::DEFAULT));
        }
    }
    rack
}

pub fn settled_index(cores: Cores, rows: &[Row], index: usize) -> Option<usize> {
    let flags = kept(cores, rows);
    if !flags.get(index).is_some_and(|kept| *kept) {
        return None;
    }
    let at = flags[..index].iter().filter(|kept| **kept).count();
    let kept = settled(cores, rows);
    let (midiless, coreless) = (cores == Cores::Required && midiless(&kept), cores == Cores::Required && coreless(&kept));
    if at >= RACK_ROWS - usize::from(midiless) - usize::from(coreless) {
        return None;
    }
    let at = at + usize::from(midiless);
    Some(at + usize::from(coreless && at >= 1))
}

pub fn pour(rack: &mut Vec<Row>, strip_cores: bool, rows: Vec<Row>) -> Vec<Option<usize>> {
    rows.into_iter()
        .map(|row| {
            if (strip_cores && row.takes_notes()) || row.times() > room(rack) || (row.is_bus() && split(rack).is_some()) {
                return None;
            }
            rack.push(row);
            Some(rack.len() - 1)
        })
        .collect()
}

#[derive(Clone, PartialEq, Debug)]
pub struct Sound {
    pub rack: Vec<Row>,
    pub lfos: Vec<LaneLfo>,
}

impl Sound {
    pub fn without_cores(self) -> Sound {
        let Sound { rack, lfos } = self;
        let kept: Vec<usize> = (0..rack.len()).filter(|&row| rack[row].is_effect()).collect();
        let lfos = moved_lfos(lfos, |row| kept.iter().position(|&index| index == row));
        let rack: Vec<Row> = rack.into_iter().filter(Row::is_effect).collect();
        Sound { rack, lfos }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub enum Preset {
    Row(Row, Vec<LaneLfo>),
    Rack(Sound),
    Chords([PaletteColumn; 7]),
}

impl Preset {
    pub const RACK: &'static str = "Rack";
    pub const CHORDS: &'static str = "Chords";

    pub fn name(&self) -> &'static str {
        match self {
            Preset::Row(row, ..) => row.label(),
            Preset::Rack(..) => Preset::RACK,
            Preset::Chords(_) => Preset::CHORDS,
        }
    }
}

pub enum Document {
    Project(Project),
    Preset(Preset),
}

impl Track {
    pub fn generates(&self) -> bool {
        !self.audio && !self.group
    }

    pub fn cores(&self) -> Cores {
        if self.generates() { Cores::Required } else { Cores::Refused }
    }

    pub fn sound(&self) -> Sound {
        Sound { rack: self.rack.clone(), lfos: self.automation.lfos.iter().filter(|lfo| lfo.aim.row().is_some()).cloned().collect() }
    }

    fn sounded(&self, mut rack: Vec<Row>) -> (Vec<Row>, bool) {
        let kept = (self.cores() == Cores::Required && !rack.iter().any(Row::is_midi)).then(|| self.rack.first().copied().filter(Row::is_midi)).flatten();
        if let Some(midi) = kept {
            rack.insert(0, midi);
        }
        (rack, kept.is_some())
    }

    pub fn wanting(&self, rack: Vec<Row>) -> usize {
        wanted(self.cores(), &self.sounded(rack).0)
    }

    pub fn take_sound(&mut self, sound: Sound) -> Vec<Option<usize>> {
        let Sound { rack, lfos } = sound;
        let cores = self.cores();
        let (rack, kept) = self.sounded(rack);
        let offset = usize::from(kept);
        let landed: Vec<Option<usize>> = (offset..rack.len()).map(|row| settled_index(cores, &rack, row)).collect();
        self.rack = settled_rack(cores, rack);
        self.automation.lfos.retain(|lfo| lfo.aim.row().is_none_or(|row| kept && row == 0));
        self.automation.lfos.extend(moved_lfos(lfos, |row| landed.get(row).copied().flatten()));
        landed
    }

    pub fn name_sound(&mut self, name: &str, description: &str) {
        self.sound = String::from(name);
        self.sound_desc = String::from(description);
    }

    pub fn forget_sound(&mut self) {
        self.sound.clear();
        self.sound_desc.clear();
    }

    pub fn first_core(&self) -> usize {
        self.rack.iter().position(Row::is_core).unwrap_or(0)
    }
}

impl Project {
    pub fn rack(&self, host: Host) -> &Vec<Row> {
        match host {
            Host::Track(track) => &self.tracks[track].rack,
            Host::Send(send) => &self.sends[send].rack,
            Host::Master => &self.master.rack,
        }
    }

    pub fn rack_mut(&mut self, host: Host) -> &mut Vec<Row> {
        match host {
            Host::Track(track) => &mut self.tracks[track].rack,
            Host::Send(send) => &mut self.sends[send].rack,
            Host::Master => &mut self.master.rack,
        }
    }

    pub fn move_row(&mut self, host: Host, from: usize, to: usize) {
        let rack = self.rack_mut(host);
        let row = rack.remove(from);
        rack.insert(to, row);
        self.automation_mut(host).move_rows(|row| Some(moved_index(row, from, to)));
    }

    pub fn remove_row(&mut self, host: Host, index: usize) {
        self.rack_mut(host).remove(index);
        self.automation_mut(host).move_rows(|row| removed_index(row, index));
    }

    pub fn cores(&self, host: Host) -> Cores {
        match host {
            Host::Track(track) => self.tracks[track].cores(),
            Host::Send(_) | Host::Master => Cores::Refused,
        }
    }
}

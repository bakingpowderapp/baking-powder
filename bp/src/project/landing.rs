use super::{Cores, Host, LaneLfo, Preset, Project, RACK_ROWS, Row, Sound, forced_ties, locked_arp, moved_lfos, pour, room, struck_at, transposed_at};
use crate::arp::{Arp, Ties};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Plan {
    Palette,
    Replaces,
    Cores,
    Joins { stripped: bool },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Keeping {
    Row(usize),
    Rack,
    Effects,
}

pub const EFFECTS_ONLY: &str = "EFFECTS ONLY";
pub const NO_ROOM: &str = "NO ROOM";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Denial {
    EffectsOnly,
    NoRoom(usize),
}

impl Denial {
    pub fn reason(self) -> &'static str {
        match self {
            Denial::EffectsOnly => EFFECTS_ONLY,
            Denial::NoRoom(_) => NO_ROOM,
        }
    }

    fn over(short: usize) -> Result<(), Denial> {
        if short == 0 { Ok(()) } else { Err(Denial::NoRoom(short)) }
    }
}

pub enum Landed {
    Sound(Vec<Option<usize>>),
    Swapped { row: usize, same: bool },
    Cores { first: usize, count: usize },
    Joined(Vec<Option<usize>>),
}

fn keep_midi<'a>(rows: impl Iterator<Item = &'a mut Row>, locked: Option<Arp>, struck: Option<u8>, mut transposed: Option<i8>, forced: Option<Ties>) {
    for row in rows {
        if let Row::Midi(midi) = row {
            midi.arp = locked.unwrap_or(midi.arp);
            midi.velocity = struck.unwrap_or(midi.velocity);
            midi.transpose = transposed.take().unwrap_or(midi.transpose);
            if let Some(ties) = forced {
                (midi.force_key, midi.ties) = (true, ties);
            }
        }
    }
}

fn cores_of(rows: Vec<Row>, lfos: Vec<LaneLfo>) -> (Vec<Row>, Vec<LaneLfo>) {
    let voiced = |row: &Row| row.takes_notes() && !row.is_bus();
    let kept: Vec<usize> = (0..rows.len()).filter(|&index| voiced(&rows[index])).collect();
    let lfos = moved_lfos(lfos, |row| kept.iter().position(|&index| index == row));
    (rows.into_iter().filter(voiced).collect(), lfos)
}

impl Preset {
    pub fn rows(&self) -> Vec<&Row> {
        match self {
            Preset::Row(row, ..) => vec![row],
            Preset::Rack(sound) => sound.rack.iter().collect(),
            Preset::Chords(_) => Vec::new(),
        }
    }
}

impl Project {
    pub fn preset_of(&self, host: Host, keeping: Keeping) -> Option<Preset> {
        let sound = || match host.track() {
            Some(track) => self.tracks[track].sound(),
            None => Sound { rack: self.rack(host).clone(), lfos: self.automation(host).lfos.iter().filter(|lfo| lfo.aim.row().is_some()).cloned().collect() },
        };
        match keeping {
            Keeping::Row(index) => {
                let row = *self.rack(host).get(index)?;
                let lfos = moved_lfos(self.automation(host).lfos.clone(), |row| (row == index).then_some(0)).into_iter().filter(|lfo| lfo.aim.row().is_some()).collect();
                Some(Preset::Row(row, lfos))
            }
            Keeping::Rack => Some(Preset::Rack(sound())),
            Keeping::Effects => Some(Preset::Rack(sound().without_cores())),
        }
    }

    pub fn plan(&self, host: Host, preset: &Preset, swapped: Option<usize>, cores_only: bool) -> Result<Plan, Denial> {
        let cores = self.cores(host);
        let rows: Vec<&Row> = match preset {
            Preset::Chords(_) => return Ok(Plan::Palette),
            Preset::Row(row, ..) if row.takes_notes() && cores == Cores::Refused => return Err(Denial::EffectsOnly),
            Preset::Row(row, ..) if row.is_core() && cores_only => return self.fitting(host, preset, Plan::Cores),
            Preset::Rack(sound) if cores_only && sound.rack.iter().any(Row::is_core) => return if cores == Cores::Refused { Err(Denial::EffectsOnly) } else { self.fitting(host, preset, Plan::Cores) },
            Preset::Rack(sound) if cores != Cores::Refused && sound.rack.iter().any(Row::is_core) => return self.fitting(host, preset, Plan::Replaces),
            Preset::Row(..) | Preset::Rack(_) => preset.rows(),
        };
        let lands = |row: &&&Row| cores != Cores::Refused || !row.takes_notes();
        let kept = rows.iter().filter(lands).count();
        if kept == 0 && !rows.is_empty() {
            return Err(Denial::EffectsOnly);
        }
        let rack = self.rack(host);
        let landing: usize = rows.iter().filter(lands).map(|row| row.times()).sum();
        let freed = swapped.filter(|_| rows.len() == 1).and_then(|row| rack.get(row)).map_or(0, Row::times);
        Denial::over(landing.max(usize::from(swapped.is_none())).saturating_sub(room(rack) + freed))?;
        Ok(Plan::Joins { stripped: kept < rows.len() })
    }

    fn fitting(&self, host: Host, preset: &Preset, plan: Plan) -> Result<Plan, Denial> {
        let rack = self.rack(host);
        let rows: Vec<Row> = preset.rows().into_iter().copied().collect();
        let after = match (plan, host) {
            (Plan::Replaces, Host::Track(track)) => self.tracks[track].wanting(rows),
            (Plan::Cores, _) => {
                let kept = usize::from(!rows.iter().any(Row::is_midi) && rack.first().is_some_and(Row::is_midi));
                let stay: usize = rack.iter().enumerate().filter(|(row, entry)| *row < kept || !entry.takes_notes()).map(|(_, entry)| entry.times()).sum();
                stay + rows.iter().filter(|row| row.takes_notes() && !row.is_bus()).map(Row::times).sum::<usize>()
            }
            _ => 0,
        };
        Denial::over(after.saturating_sub(RACK_ROWS))?;
        Ok(plan)
    }

    pub fn land(&mut self, host: Host, preset: Preset, plan: Plan, swapped: Option<usize>, named: (&str, &str)) -> Landed {
        let names_sound = matches!(preset, Preset::Rack(_) | Preset::Row(Row::Core(_), ..));
        let (mut rows, lfos) = match preset {
            Preset::Chords(_) => return Landed::Joined(Vec::new()),
            Preset::Row(row, lfos) => (vec![row], lfos),
            Preset::Rack(Sound { rack, lfos }) => (rack, lfos),
        };
        rows.iter_mut().for_each(|row| *row = row.renumbered(|_| None));
        let rack = self.rack(host);
        let transposed = match (plan, swapped) {
            (Plan::Replaces | Plan::Cores, _) => transposed_at(rack),
            (_, Some(row)) => rack.get(row..).and_then(transposed_at),
            _ => None,
        };
        keep_midi(rows.iter_mut(), locked_arp(rack), struck_at(rack), transposed, forced_ties(rack));
        let landed = match (plan, host, swapped) {
            (Plan::Replaces, Host::Track(track), _) => Landed::Sound(self.tracks[track].take_sound(Sound { rack: rows, lfos })),
            (Plan::Cores, ..) => {
                let (cores, lfos) = cores_of(rows, lfos);
                let (first, count) = self.replace_cores(host, cores, lfos);
                Landed::Cores { first, count }
            }
            (_, _, Some(row)) if rows.len() == 1 => {
                let same = self.rack(host)[row].same_kind(&rows[0]);
                self.replace_row(host, row, rows.remove(0), lfos);
                Landed::Swapped { row, same }
            }
            _ => {
                let strip_cores = self.cores(host) == Cores::Refused;
                let landed = pour(self.rack_mut(host), strip_cores, rows);
                self.automation_mut(host).lfos.extend(moved_lfos(lfos, |at| landed.get(at).copied().flatten()));
                Landed::Joined(landed)
            }
        };
        if names_sound && let Some(track) = host.track() {
            self.tracks[track].name_sound(named.0, named.1);
        }
        landed
    }

    fn replace_row(&mut self, host: Host, index: usize, mut row: Row, lfos: Vec<LaneLfo>) {
        let rack = self.rack_mut(host);
        if let (Row::Effect(old), Row::Effect(new)) = (&rack[index], &mut row)
            && let Some((source, pre)) = old.wiring()
        {
            new.wire(source, pre);
        }
        rack[index] = row;
        let lanes = self.automation_mut(host);
        lanes.lfos.retain(|lfo| lfo.aim.row() != Some(index));
        lanes.lfos.extend(moved_lfos(lfos, |at| (at == 0).then_some(index)));
    }

    fn replace_cores(&mut self, host: Host, cores: Vec<Row>, lfos: Vec<LaneLfo>) -> (usize, usize) {
        let rack = self.rack(host);
        let kept = usize::from(!cores.iter().any(Row::is_midi) && rack.first().is_some_and(Row::is_midi));
        let first = rack.iter().skip(kept).position(Row::takes_notes).map_or(kept, |at| at + kept);
        let old: Vec<usize> = (kept..rack.len()).filter(|&row| rack[row].takes_notes()).collect();
        for index in old.into_iter().rev() {
            self.remove_row(host, index);
        }
        let count = cores.len().min(RACK_ROWS - self.rack(host).len());
        for (offset, core) in cores.into_iter().take(count).enumerate() {
            let rack = self.rack_mut(host);
            rack.push(core);
            let last = rack.len() - 1;
            self.move_row(host, last, first + offset);
        }
        self.automation_mut(host).lfos.extend(moved_lfos(lfos, |row| (row < count).then_some(first + row)));
        (first, count)
    }
}

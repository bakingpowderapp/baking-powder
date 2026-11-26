use super::*;
use super::automation::Armed;
use super::edit::{Clipboard, Copied, Setting};

const MATCH_ALL: &str = "MATCH ALL";
const RACK: &str = "RACK";
const BACK_TO_ONE: &str = "BACK TO 1";
const FLATTEN: &str = "FLATTEN";
const COPIED: &str = "EFFECTS SPLIT";
const MUTED: &str = "PRINTED MUTED";

fn lanes_at(copied: &Copied, at: usize) -> (Vec<Curve>, Vec<LaneLfo>) {
    let here = |_| Some(at);
    (copied.curves.iter().cloned().filter_map(|mut curve| curve.aim.remap(&here).then_some(curve)).collect(), moved_lfos(copied.lfos.clone(), here))
}

impl App {
    pub(super) fn flatten(&mut self, host: Host, row: usize) {
        if self.project.rack(host)[row].is_effect() {
            return self.flatten_duck(host, row);
        }
        let Host::Track(index) = host else {
            return;
        };
        let sent = audio::sent(&self.project);
        let name = self.project.tracks[index].name.clone();
        match flatten::row(&mut self.project, index, row, &sent) {
            flatten::Flattened::Split { removed, copied, muted } => {
                self.inserted(index + 1, &[MIDI], 1);
                for at in (0..RACK_ROWS).rev().filter(|at| removed & (1 << at) != 0) {
                    self.row_removed(host, at);
                }
                self.select(index + 1);
                let reason = if muted { Some(MUTED) } else if copied { Some(COPIED) } else { None };
                self.notice = reason.map(|reason| Notice { band: FLATTEN, name, reason });
            }
            flatten::Flattened::Printed | flatten::Flattened::Nothing => {}
        }
    }

    pub(super) fn match_all(&mut self, host: Host, row: usize) {
        let Some(Row::Effect(Slot { params: fx::Params::Sidechain(like), .. })) = self.project.rack(host).get(row).copied() else {
            return;
        };
        let others = self.project.match_sidechains(host, row).map_or(0, |matched| matched.len() as i32);
        let source = self.track_names().get(like.signal.source as usize).cloned().unwrap_or_default();
        self.notice = Some(if others > 0 { Notice { band: MATCH_ALL, name: format!("{} ON {source}", counted(others, "SIDECHAIN")), reason: "MATCHED" } } else { Notice { band: MATCH_ALL, name: source, reason: "NO OTHERS" } });
    }

    pub(super) fn rack_action(&mut self, action: Action) {
        if action == Action::Insert(Pick::Paste) {
            if let Clipboard::Rows { rows, song, .. } = self.clipboard.clone() {
                self.paste_rows(&rows, song);
            }
            return;
        }
        let host = self.host();
        let allowed = self.project.cores(host);
        let inserted = match action {
            Action::Insert(Pick::Midi) if allowed != Cores::Refused => Some(Row::Midi(arp::MidiRow::DEFAULT)),
            Action::Insert(Pick::Core) if allowed != Cores::Refused => Some(Row::Core(Layer::DEFAULT)),
            Action::Insert(Pick::Effect(kind)) => Some(self.fresh_effect(kind)),
            _ => None,
        };
        let rack = self.project.rack_mut(host);
        let cores = rack.iter().filter(|row| row.is_core()).count();
        match action {
            Action::Open(row) => {
                let held = self.held_strip();
                self.open = Some(row);
                self.face.close();
                self.picture.reset();
                self.regain_strip(held);
            }
            Action::Insert(pick) => {
                let Some(row) = inserted else {
                    return;
                };
                let Some(at) = self.land_rows(vec![row]).first().copied() else {
                    return;
                };
                let at = if pick == Pick::Midi { self.lead_bus(at) } else { at };
                self.pick_rows(vec![at]);
                self.rack_action(Action::Open(at));
            }
            Action::Move(from, to) => {
                if from.min(to) == 0 && rack[0].is_midi() {
                    return;
                }
                self.project.move_row(host, from, to);
                self.open = self.open.map(|open| moved_index(open, from, to));
                self.rack.moved_row(from, to);
                self.lfo_face.close();
                self.lfo_strip = None;
                if let Some(armed) = &mut self.armed {
                    armed.aim.remap(&|row| Some(moved_index(row, from, to)));
                }
                if let Some(track) = host.track() {
                    let generator = self.generator_mut(track);
                    if let Some(solo) = &mut generator.core {
                        solo.row = moved_index(solo.row, from, to);
                    }
                }
            }
            Action::Remove(index) => {
                if (rack[index].is_core() && cores <= 1 && allowed == Cores::Required) || (index == 0 && rack[index].is_midi()) {
                    return;
                }
                let removed_core = rack[index].is_core();
                self.project.remove_row(host, index);
                tidy(self.project.rack_mut(host));
                if removed_core && let Some(track) = host.track() {
                    self.project.tracks[track].forget_sound();
                }
                self.row_removed(host, index);
            }
            Action::RemovePicked => {
                self.delete_picked_rows();
            }
            Action::Land(slot) => {
                let (row, last) = (rack[slot], rack.len() - 1);
                let Hover::Button(Button::Send(target)) = self.hover else {
                    return;
                };
                let send = self.send_ordinal(target);
                if self.send_short(send, slot) > 0 {
                    return;
                }
                if Host::Send(send) == host {
                    if slot < last {
                        self.rack_action(Action::Move(slot, last));
                    }
                    return;
                }
                let copied = self.copied_row(host, slot).filter(|_| !self.project.lock);
                self.rack_action(Action::Remove(slot));
                let landed = Host::Send(send);
                self.project.rack_mut(landed).push(row);
                if let Some(copied) = copied {
                    self.carry_lanes(landed, self.project.rack(landed).len() - 1, &copied);
                }
            }
            Action::Mix(row) => {
                let Some(mixed) = rack[row].mix_aim(row) else {
                    return;
                };
                self.arm(Armed::of(mixed));
            }
            Action::Restore(row) => {
                let Some(mixed) = rack[row].mix_aim(row) else {
                    return;
                };
                if !self.project.lock && self.curve_exists(host, &mixed) {
                    self.delete_curve(host, &mixed);
                } else {
                    self.project.rack_mut(host)[row].set_mix(1.0);
                }
            }
            Action::Solo(row) => {
                if let Some(track) = host.track() {
                    let generator = self.generator_mut(track);
                    generator.core = match generator.core {
                        Some(solo) if solo.row == row => (!solo.effects).then_some(CoreSolo { row, effects: true }),
                        _ => Some(CoreSolo { row, effects: false }),
                    };
                }
            }
            Action::Times(row) => {
                let (times, stopped) = next_times(rack, row);
                let dropped = times < rack[row].times();
                rack[row].set_times(times);
                if stopped {
                    self.notice = Some(Notice { band: RACK, name: needing(1, ROW), reason: if dropped { BACK_TO_ONE } else { NO_ROOM } });
                }
            }
        }
    }

    fn row_removed(&mut self, host: Host, index: usize) {
        self.open = match self.open {
            Some(open) if open > index => Some(open - 1),
            Some(open) if open == index => None,
            open => open,
        };
        self.rack.removed_row(index);
        self.face.close();
        self.lfo_face.close();
        self.lfo_strip = None;
        self.picture.reset();
        if let Some(armed) = &mut self.armed
            && !armed.aim.remap(&|row| removed_index(row, index))
        {
            self.armed = None;
        }
        if let Some(track) = host.track() {
            let generator = self.generator_mut(track);
            generator.core = match generator.core {
                Some(solo) if solo.row == index => None,
                Some(solo) if solo.row > index => Some(CoreSolo { row: solo.row - 1, ..solo }),
                core => core,
            };
        }
    }

    pub(super) fn offer(&self) -> Offer {
        Offer { cores: self.project.cores(self.host()), paste: matches!(self.clipboard, Clipboard::Rows { .. }) }
    }

    pub(super) fn picked_rows(&self) -> Vec<Copied> {
        if !self.core_shown() {
            return Vec::new();
        }
        let host = self.host();
        self.rack.picked().into_iter().filter_map(|index| self.copied_row(host, index)).collect()
    }

    fn copied_row(&self, host: Host, index: usize) -> Option<Copied> {
        let row = *self.project.rack(host).get(index)?;
        let lanes = self.project.automation(host);
        let curves = lanes.curves.iter().filter(|curve| curve.aim.row() == Some(index)).cloned().collect();
        let lfos = lanes.lfos.iter().filter(|lfo| lfo.aim.row() == Some(index)).cloned().collect();
        Some(Copied { row, curves, lfos })
    }

    fn carry_lanes(&mut self, host: Host, at: usize, copied: &Copied) {
        let (owner, (curves, lfos)) = (self.project.automation_mut(host), lanes_at(copied, at));
        owner.curves.extend(curves);
        owner.lfos.extend(lfos);
    }

    pub(super) fn pick_all_rows(&mut self) -> bool {
        self.core_shown() && self.rack.pick_all(self.project.rack(self.host()))
    }

    pub(super) fn pick_rows(&mut self, rows: Vec<usize>) {
        self.rack.pick(self.project.rack(self.host()), rows);
    }

    fn land_rows(&mut self, rows: Vec<Row>) -> Vec<usize> {
        let (host, bus, lone) = (self.host(), self.rack.bus(), self.lone());
        if self.crowds(taken(&rows) + usize::from(lone), 0) {
            return Vec::new();
        }
        let rack = self.project.rack_mut(host);
        if lone {
            rack.push(Row::Bus(Bus::DEFAULT));
        }
        let first = rack.len();
        rack.extend(rows.into_iter().map(Row::unseeded));
        let (count, at) = (rack.len() - first, split(rack).filter(|_| bus == 0).unwrap_or(first));
        for moved in 0..count {
            if at != first {
                self.rack_action(Action::Move(first + moved, at + moved));
            }
        }
        (at..at + count).collect()
    }

    fn lone(&self) -> bool {
        let rack = self.project.rack(self.host());
        self.rack.bus() > 0 && split(rack).is_none() && rack.first().is_some_and(Row::is_midi)
    }

    fn crowds(&mut self, landing: usize, freed: usize) -> bool {
        let short = landing.saturating_sub(room(self.project.rack(self.host())) + freed);
        if short > 0 {
            self.notice = Some(Notice { band: RACK, name: needing(short, ROW), reason: NO_ROOM });
        }
        short > 0
    }

    fn lead_bus(&mut self, row: usize) -> usize {
        let top = crate::project::shown(self.project.rack(self.host()), self.rack.bus()).start;
        if row != top {
            self.rack_action(Action::Move(row, top));
        }
        top
    }

    pub(super) fn paste_rows(&mut self, rows: &[Copied], song: u64) -> bool {
        let host = self.host();
        let allowed = self.project.cores(host);
        let carry = !self.project.lock;
        let mut kept: Vec<&Copied> = rows.iter().filter(|copied| !copied.row.takes_notes() || allowed != Cores::Refused).collect();
        let over = kept.iter().position(|copied| copied.row.is_midi()).filter(|&at| self.overwrites(host, kept[at], carry));
        let landing = kept.iter().map(|copied| copied.row.times()).sum::<usize>() + usize::from(self.lone() && kept.len() > usize::from(over.is_some()));
        if self.crowds(landing, over.map_or(0, |_| self.project.rack(host)[0].times())) {
            return true;
        }
        let master = over.map(|at| self.overwrite(host, kept.remove(at), carry));
        let here = song == self.disk.song;
        let signalled: Vec<Row> = kept.iter().map(|copied| if here { self.project.signalled_on(host, copied.row) } else { copied.row.renumbered(|_| None) }).collect();
        let mut pasted = if signalled.is_empty() { Vec::new() } else { self.land_rows(signalled) };
        if carry {
            for (at, copied) in pasted.iter().zip(&kept) {
                self.carry_lanes(host, *at, copied);
            }
        }
        for at in (0..pasted.len()).rev().filter(|&at| kept[at].row.is_midi()) {
            let from = pasted[at];
            let to = self.lead_bus(from);
            pasted.iter_mut().for_each(|row| *row = moved_index(*row, from, to));
        }
        let picked: Vec<usize> = master.into_iter().chain(pasted).collect();
        let Some(&last) = picked.last() else {
            return false;
        };
        self.pick_rows(picked);
        self.rack_action(Action::Open(last));
        true
    }

    fn overwrites(&self, host: Host, copied: &Copied, carry: bool) -> bool {
        let Some(held) = self.copied_row(host, 0).filter(|held| held.row.is_midi() && self.rack.picked().contains(&0)) else {
            return false;
        };
        let (curves, lfos) = lanes_at(copied, 0);
        held.row != copied.row || (carry && (held.curves != curves || held.lfos != lfos))
    }

    fn overwrite(&mut self, host: Host, copied: &Copied, carry: bool) -> usize {
        self.project.rack_mut(host)[0] = copied.row;
        if carry {
            let lanes = self.project.automation_mut(host);
            lanes.curves.retain(|curve| curve.aim.row() != Some(0));
            lanes.lfos.retain(|lfo| lfo.aim.row() != Some(0));
            self.carry_lanes(host, 0, copied);
        }
        0
    }

    pub(super) fn delete_picked_rows(&mut self) -> bool {
        if !self.core_shown() {
            return false;
        }
        let picked = self.rack.picked();
        if picked.is_empty() {
            return false;
        }
        for row in picked.into_iter().rev() {
            self.rack_action(Action::Remove(row));
        }
        true
    }

    pub(super) fn bypass_picked_rows(&mut self) -> bool {
        if !self.core_shown() {
            return false;
        }
        let host = self.host();
        let mut rows = self.rack.picked();
        if rows.is_empty() {
            rows.extend(self.shown().map(|(_, row)| row));
        }
        let rack = self.project.rack_mut(host);
        rows.retain(|&row| rack.get(row).is_some_and(|entry| !entry.is_core()));
        if rows.is_empty() {
            return false;
        }
        let bypass = !rows.iter().all(|&row| rack[row].bypassed());
        for row in rows {
            rack[row].set_bypass(bypass);
        }
        true
    }

    pub(super) fn shown_arp(&self) -> Option<&arp::Arp> {
        let (host, row) = self.shown_midi().filter(|_| self.lanes_shown())?;
        match &self.project.rack(host)[row] {
            Row::Midi(midi) => Some(&midi.arp),
            Row::Core(_) | Row::Effect(_) | Row::Bus(_) => None,
        }
    }

    pub(super) fn edit_arp(&mut self, edit: impl FnOnce(&mut Lanes, &mut arp::Arp) -> bool) -> bool {
        let Some((host, row)) = self.shown_midi().filter(|_| self.lanes_shown()) else {
            return false;
        };
        match &mut self.project.rack_mut(host)[row] {
            Row::Midi(midi) => edit(&mut self.lanes, &mut midi.arp),
            Row::Core(_) | Row::Effect(_) | Row::Bus(_) => false,
        }
    }

    pub(super) fn edit_pattern(&mut self, edit: impl FnOnce(&mut Lanes, &mut arp::Pattern) -> bool) -> bool {
        self.edit_arp(|lanes, arp| edit(lanes, &mut arp.pattern))
    }

    pub(super) fn picked_setting(&self) -> Option<Setting> {
        let (host, row) = self.shown().filter(|_| self.core_shown())?;
        let index = self.face.picked()?;
        let (band, param, value) = self.project.rack(host)[row].faced().setting_of(index)?;
        let aim = Aim::knob(row, band, param);
        let lanes = self.project.automation(host);
        let curves: Vec<Curve> = lanes.curves.iter().filter(|curve| curve.aim == aim).cloned().collect();
        let swayed = curves.iter().any(|curve| curve.lfo.is_some());
        let lfo = lanes.lfos.iter().find(|lfo| lfo.aim == aim).filter(|lfo| swayed || lfo.amount > 0.0).cloned();
        Some(Setting { band, param, value, curves, lfo })
    }

    pub(super) fn paste_setting(&mut self, setting: &Setting) -> bool {
        let Some((host, row)) = self.shown() else {
            return false;
        };
        let (band, param) = (setting.band, setting.param);
        let Some(index) = self.project.rack_mut(host)[row].faced_mut().assign(band, param, setting.value) else {
            return false;
        };
        let aim = Aim::knob(row, band, param);
        if !self.project.lock && self.automatable(host, &aim) {
            let held = self.held_strip();
            let lanes = self.project.automation_mut(host);
            lanes.curves.retain(|curve| curve.aim != aim);
            lanes.remove_lfo(&aim);
            lanes.curves.extend(setting.curves.iter().map(|curve| Curve { aim: aim.clone(), ..curve.clone() }));
            if let Some(lfo) = &setting.lfo {
                let entry = lanes.lfo_mut(&aim);
                *entry = LaneLfo { aim, ..lfo.clone() };
                entry.set_audio(lfo.audio);
            }
            self.regain_strip(held);
        }
        self.face.pick(index);
        self.rack.unpick();
        true
    }

    pub(super) fn delete_picked_cell(&mut self) -> bool {
        let Some((host, row)) = self.shown().filter(|_| self.core_shown()) else {
            return false;
        };
        let Some(index) = self.face.picked() else {
            return false;
        };
        self.reset_cell(host, row, index)
    }

    pub(super) fn nudge_cell(&mut self, key: Key, shift: bool) -> bool {
        let Some((host, row)) = self.shown().filter(|_| self.core_shown() && self.face.picking()) else {
            return false;
        };
        let Some(direction) = key.vertical() else {
            return false;
        };
        let bpm = self.project.bpm;
        self.project.rack_mut(host)[row].faced_mut().nudge(&mut self.face, direction, shift, bpm)
    }
}

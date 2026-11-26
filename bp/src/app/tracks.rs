use super::*;
use super::files::ADD_ITEMS;
use super::region::{Covered, Region};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum TrackButton {
    Delete,
    Solo,
    Mute,
    Add,
}

pub(super) const TRACK_BUTTONS: [(TrackButton, &str); 4] = [
    (TrackButton::Delete, "X"),
    (TrackButton::Solo, "S"),
    (TrackButton::Mute, "M"),
    (TrackButton::Add, "+"),
];

#[derive(Clone)]
pub(super) struct Gang {
    pub(super) origin_y: i32,
    pub(super) origins: Vec<(usize, f32)>,
}

impl Gang {
    pub(super) fn holds(&self, track: usize) -> bool {
        self.origins.iter().any(|&(held, _)| held == track)
    }
}

#[derive(Clone, Copy)]
pub(super) struct MasterDrag {
    pub(super) origin_y: i32,
    pub(super) origin: f32,
}

#[derive(Clone)]
pub(super) struct SendDrag {
    pub(super) send: usize,
    pub(super) gang: Gang,
    pub(super) moved: bool,
}

#[derive(Clone)]
pub(super) struct TrackMarquee {
    pub(super) origin: (i32, i32),
    pub(super) at: (i32, i32),
    pub(super) armed: bool,
    pub(super) hit: Option<usize>,
    pub(super) before: Vec<usize>,
}

#[derive(Clone, Copy)]
pub(super) struct TrackDrag {
    pub(super) track: usize,
    pub(super) grab: i32,
    pub(super) origin_y: i32,
    pub(super) y: i32,
    pub(super) armed: bool,
    pub(super) title: bool,
}

#[derive(Clone, Copy)]
pub(super) struct DropSpot {
    pub(super) before: Option<usize>,
    pub(super) slot: usize,
    pub(super) inside: bool,
    pub(super) kept: bool,
}

remembered!(Selection {
    whole track: usize = 0, "selected", 0..=i32::MAX;
    hand track_picks: Vec<usize> = Vec::new();
    hand clips: Vec<(usize, i32)> = Vec::new();
    hand paste_point: Option<(usize, i32)> = None;
    state region: Option<Region> = None;
});

impl Selection {
    pub(super) fn settle(&mut self, tracks: usize) {
        self.track = self.track.min(tracks - 1);
        self.track_picks.retain(|track| *track < tracks && *track != self.track);
    }

    pub(super) fn replace(&mut self) {
        self.track = 0;
        self.paste_point = None;
        self.clear();
    }

    pub(super) fn clear(&mut self) {
        self.clips.clear();
        self.track_picks.clear();
        self.region = None;
    }

    pub(super) fn renumber(&mut self, map: impl Fn(usize) -> Option<usize>) {
        let place = |slot: &mut usize| match map(*slot) {
            Some(moved) => {
                *slot = moved;
                true
            }
            None => false,
        };
        self.track_picks.retain_mut(place);
        self.clips.retain_mut(|(track, _)| place(track));
        self.paste_point.take_if(|(track, _)| !place(track));
        if let Some(region) = &mut self.region {
            region.covers.retain_mut(|covered| match covered {
                Covered::Track(track) | Covered::Curve(Host::Track(track), _) => place(track),
                Covered::Curve(Host::Send(_) | Host::Master, _) => true,
            });
        }
        self.region.take_if(|region| region.covers.is_empty());
    }

    pub(super) fn picked_tracks(&self) -> Vec<usize> {
        let mut picked = self.track_picks.clone();
        picked.push(self.track);
        picked.sort_unstable();
        picked.dedup();
        picked
    }

    pub(super) fn pick_of(&self, track: usize) -> Vec<usize> {
        let picked = self.picked_tracks();
        if picked.contains(&track) { picked } else { vec![track] }
    }

    pub(super) fn region_range(&self) -> Option<Loop> {
        self.region.as_ref().map(Region::range)
    }
}

pub(super) fn insert_at<T: Default>(list: &mut Vec<T>, at: usize) {
    if at <= list.len() {
        list.insert(at, T::default());
    }
}

pub(super) fn remove_at<T>(list: &mut Vec<T>, at: usize) {
    if at < list.len() {
        list.remove(at);
    }
}

pub(super) fn move_at<T: Default>(list: &mut Vec<T>, from: usize, to: usize) {
    if from < list.len() {
        let entry = list.remove(from);
        if list.len() < to {
            list.resize_with(to, T::default);
        }
        list.insert(to, entry);
    }
}

impl App {
    pub(super) fn track_drag(&self) -> Option<TrackDrag> {
        match self.drag {
            Some(Drag::Track(drag)) => Some(drag),
            _ => None,
        }
    }

    pub(super) fn select(&mut self, track: usize) {
        self.leave();
        let from = self.host();
        let switched = from != Host::Track(track);
        self.carry_armed(from, Host::Track(track));
        self.selection.track = track;
        self.monitored = None;
        if switched {
            self.arrive();
            self.settle_armed();
        }
        self.view.palette_shown = false;
        self.selection.clear();
    }

    pub(super) fn select_host(&mut self, host: Host) {
        if self.monitored != Some(host) {
            self.leave();
            let from = self.host();
            self.carry_armed(from, host);
            self.monitored = Some(host);
            self.show(None);
            self.settle_armed();
        }
        self.view.palette_shown = false;
        self.selection.clips.clear();
    }

    pub(super) fn unmonitor(&mut self) {
        let from = self.host();
        self.carry_armed(from, Host::Track(self.selection.track));
        self.monitored = None;
        self.arrive();
        self.settle_armed();
    }

    pub(super) fn toggle_master(&mut self) {
        if self.monitored == Some(Host::Master) {
            self.unmonitor();
        } else {
            self.select_host(Host::Master);
        }
    }

    pub(super) fn move_track(&mut self, from: usize, to: usize) {
        self.project.move_track(from, to);
        if let Some(audio) = &mut self.audio {
            audio.move_track(from, to);
        }
        self.selection.track = moved_index(self.selection.track, from, to);
        move_at(&mut self.generators, from, to);
        self.renumber(|track| Some(moved_index(track, from, to)));
    }

    pub(super) fn renumber(&mut self, map: impl Fn(usize) -> Option<usize>) {
        self.clipboard.renumber(self.disk.song, &map);
        self.selection.renumber(&map);
        let place = |slot: &mut usize| match map(*slot) {
            Some(moved) => {
                *slot = moved;
                true
            }
            None => false,
        };
        self.live.keys.retain_mut(|note| place(&mut note.track));
        self.live.sounding.retain_mut(|(track, _)| place(track));
        self.pool.renumber(place);
        self.menu.take_if(|menu| match menu {
            Menu::Add(track) | Menu::Follow(track) => !place(track),
            _ => false,
        });
        self.rename.take_if(|rename| !place(&mut rename.track));
        if let Some(latch) = &mut self.live.latched {
            latch.tracks.retain_mut(place);
        }
        self.live.latched.take_if(|latch| latch.tracks.is_empty());
    }

    pub(super) fn dragged_tracks(&self, drag: TrackDrag) -> Vec<usize> {
        self.with_members(self.selection.pick_of(drag.track))
    }

    pub(super) fn drop_track(&mut self, drag: TrackDrag) {
        let spot = self.drop_spot(drag);
        if spot.kept {
            return;
        }
        let moving = self.dragged_tracks(drag);
        let members: Vec<bool> = moving.iter().map(|&track| self.project.group_of(track).is_some_and(|group| moving.contains(&group))).collect();
        let before = spot.before.unwrap_or(self.project.tracks.len());
        let above = moving.iter().filter(|&&track| track < before).count();
        let settled = moving[..above].iter().rev().zip((0..before).rev()).take_while(|&(&track, at)| track == at).count();
        let sinking = above - settled;
        for (sunk, &track) in moving[..sinking].iter().enumerate() {
            self.move_track(track - sunk, before - settled - 1);
        }
        for (risen, &track) in moving[above..].iter().enumerate() {
            self.move_track(track, before + risen);
        }
        let first = before - above;
        for (offset, member) in members.into_iter().enumerate() {
            let track = &mut self.project.tracks[first + offset];
            if !track.group {
                track.inside = member || spot.inside;
            }
        }
    }

    pub(super) fn remove_track(&mut self, index: usize) -> bool {
        let held = self.project.tracks.get(index).cloned();
        if !self.project.remove_track(index) {
            return false;
        }
        if let Some(held) = held {
            self.clipboard.lose(self.disk.song, index, &held);
        }
        let selected_gone = index == self.selection.track;
        remove_at(&mut self.generators, index);
        if let Some(audio) = &mut self.audio {
            audio.remove(index);
        }
        self.renumber(|track| (track != index).then(|| track - usize::from(track > index)));
        if self.selection.track > index || self.selection.track >= self.project.tracks.len() {
            self.selection.track -= 1;
        }
        self.selection.track_picks.retain(|track| *track != self.selection.track);
        if selected_gone {
            if self.monitored.is_none() {
                self.armed = self.generator(self.selection.track).armed;
                self.arrive();
            }
            self.settle_armed();
        }
        true
    }

    pub(super) fn send_ordinal(&self, slot: usize) -> usize {
        self.view.bank * SENDS_PER_BANK + slot
    }

    pub(super) fn send_fed(&self, send: usize) -> bool {
        self.project.tracks.iter().any(|track| track.sends[send] > 0.0)
    }

    pub(super) fn send_short(&self, send: usize, row: usize) -> usize {
        let host = self.host();
        let landing = if Host::Send(send) == host { 0 } else { self.project.rack(host).get(row).map_or(1, Row::times) };
        landing.saturating_sub(room(&self.project.sends[send].rack))
    }

    pub(super) fn send_lit(&self, send: usize) -> bool {
        self.soloed == Some(send) || self.monitored == Some(Host::Send(send))
    }

    pub(super) fn master_gain(&self) -> f32 {
        self.project.gain_of(self.panel_host())
    }

    pub(super) fn set_master_gain(&mut self, gain: f32) -> bool {
        let target = match self.panel_host() {
            Host::Send(send) => &mut self.project.sends[send].gain,
            _ => &mut self.project.master.gain,
        };
        let changed = *target != gain;
        *target = gain;
        changed
    }

    pub(super) fn tap_send(&mut self, send: usize) {
        if self.soloed == Some(send) {
            self.soloed = None;
            if self.monitored == Some(Host::Send(send)) {
                self.unmonitor();
            }
        } else if self.monitored == Some(Host::Send(send)) {
            self.soloed = Some(send);
        } else {
            self.select_host(Host::Send(send));
        }
    }

    pub(super) fn pick_track(&mut self, index: usize) {
        if index == self.selection.track {
            return;
        }
        if let Some(at) = self.selection.track_picks.iter().position(|picked| *picked == index) {
            self.selection.track_picks.remove(at);
            return;
        }
        let mut picks = std::mem::take(&mut self.selection.track_picks);
        picks.push(self.selection.track);
        self.select(index);
        self.selection.track_picks = picks;
    }

    pub(super) fn gang(&self, track: usize, origin_y: i32, origin: impl Fn(&Track) -> f32) -> Gang {
        let origins = self.selection.pick_of(track).into_iter().map(|picked| (picked, origin(&self.project.tracks[picked]))).collect();
        Gang { origin_y, origins }
    }

    pub(super) fn insert_track(&mut self, at: usize, track: Track, joins: bool) -> bool {
        let kind = [track.kind()];
        let placed = if joins { self.project.add_track(at, track) } else { self.project.add_track_outside(at, track) };
        self.inserted(at, &kind, usize::from(placed)) > 0
    }

    pub(super) fn paste_tracks(&mut self, at: usize, tracks: Vec<Track>, homes: &[Option<usize>], song: u64) -> usize {
        let kinds: Vec<&'static str> = tracks.iter().map(Track::kind).collect();
        let added = if song == self.disk.song { self.project.insert_tracks(at, tracks) } else { self.project.insert_foreign_tracks(at, tracks, homes) };
        self.inserted(at, &kinds, added)
    }

    pub(super) fn inserted(&mut self, at: usize, kinds: &[&'static str], added: usize) -> usize {
        for track in at..at + added {
            insert_at(&mut self.generators, track);
            if let Some(audio) = &mut self.audio {
                audio.insert(track, &self.project.tracks[track]);
            }
            self.renumber(|moved| Some(if moved >= track { moved + 1 } else { moved }));
            self.selection.track += usize::from(self.selection.track >= track);
        }
        if let Some(kind) = kinds.get(added) {
            self.notice = Some(Notice { band: TRACK, name: lacking(kind, kinds.len() - added, TRACK), reason: NO_ROOM });
        }
        added
    }

    pub(super) fn delete_picked_tracks(&mut self) -> bool {
        let mut removed = false;
        for track in self.selection.picked_tracks().into_iter().rev() {
            removed |= self.remove_track(track);
        }
        removed
    }

    pub(super) fn with_members(&self, tracks: Vec<usize>) -> Vec<usize> {
        let mut all = tracks;
        for track in all.clone() {
            all.extend(self.project.members_of(track));
        }
        all.sort_unstable();
        all.dedup();
        all
    }

    pub(super) fn group_or_ungroup(&mut self) -> bool {
        let picked = self.selection.picked_tracks();
        if let [group] = picked[..]
            && self.project.tracks[group].group
        {
            return self.remove_track(group);
        }
        if picked.iter().any(|&track| self.project.tracks[track].group) {
            return false;
        }
        if self.all_grouped(&picked) {
            for &track in picked.iter().rev() {
                self.leave_group(track);
            }
            return true;
        }
        let Some(&first) = picked.first() else {
            return false;
        };
        if !self.insert_track(first, Track::group(), true) {
            return true;
        }
        for (offset, &track) in picked.iter().enumerate() {
            let member = first + 1 + offset;
            if track + 1 != member {
                self.move_track(track + 1, member);
            }
            self.project.tracks[member].inside = true;
        }
        let mut head = first;
        while self.project.tracks.get(head + picked.len() + 1).is_some_and(|track| track.inside) {
            self.move_track(head + picked.len() + 1, head);
            head += 1;
        }
        self.select(head);
        true
    }

    pub(super) fn all_grouped(&self, tracks: &[usize]) -> bool {
        tracks.iter().all(|&track| self.project.group_of(track).is_some())
    }

    fn leave_group(&mut self, track: usize) {
        let Some(last) = self.project.group_of(track).and_then(|group| self.project.members_of(group).last().copied()) else {
            return;
        };
        if track != last {
            self.move_track(track, last);
        }
        self.project.tracks[last].inside = false;
    }

    pub(super) fn toggle_fold(&mut self, group: usize) -> bool {
        let folded = !self.project.tracks[group].folded;
        self.project.tracks[group].folded = folded;
        if folded {
            if self.project.group_of(self.selection.track) == Some(group) {
                let picked = std::mem::take(&mut self.selection.clips);
                self.select(group);
                self.selection.clips = picked;
            }
            self.selection.track_picks.retain(|&track| self.project.group_of(track) != Some(group));
        }
        true
    }

    pub(super) fn sweep_tracks(&mut self, x: i32, y: i32, hit: Option<usize>) {
        self.drag = Some(Drag::TrackMarquee(TrackMarquee { origin: (x, y), at: (x, y), armed: false, hit, before: self.selection.track_picks.clone() }));
    }

    pub(super) fn column_bounds(&self) -> Rect {
        let viewport = self.track_viewport();
        Rect::new(viewport.x, viewport.y, column_w() - viewport.x, viewport.h)
    }

    pub(super) fn swept_tracks(&self, sweep: Rect) -> Vec<usize> {
        let sweep = sweep.intersect(self.column_bounds());
        self.track_slots().into_iter().filter(|&(slot, _)| self.track_tile(slot).overlaps(sweep)).map(|(_, track)| track).collect()
    }

    pub(super) fn tile_down(&mut self, index: usize, x: i32, y: i32) {
        if self.shift {
            self.sweep_tracks(x, y, Some(index));
            return;
        }
        let tile = self.track_tile(self.track_slot(index));
        self.drag = Some(Drag::Track(TrackDrag { track: index, grab: y - tile.y, origin_y: y, y, armed: false, title: tile_title(tile).contains(x, y) }));
    }

    pub(super) fn volume_down(&mut self, index: usize, y: i32, reset: bool) -> bool {
        if !reset {
            self.drag = Some(Drag::Volume(self.gang(index, y, |picked| gain_to_position(picked.gain))));
            return true;
        }
        for picked in self.selection.pick_of(index) {
            if !self.project.lock && self.curve_exists(Host::Track(picked), &Aim::Gain) {
                self.delete_curve(Host::Track(picked), &Aim::Gain);
            } else {
                self.project.tracks[picked].gain = 1.0;
            }
        }
        true
    }

    pub(super) fn delay_down(&mut self, track: usize, y: i32, reset: bool) {
        self.menu = Some(Menu::Add(track));
        if !reset {
            self.drag = Some(Drag::Delay(self.gang(track, y, |picked| picked.delay_ms)));
            return;
        }
        for picked in self.selection.pick_of(track) {
            self.project.tracks[picked].delay_ms = 0.0;
        }
    }

    pub(super) fn master_down(&mut self, y: i32, reset: bool) -> bool {
        if let Some(audio) = &mut self.audio {
            audio.clear_clip();
        }
        if !reset {
            self.drag = Some(Drag::Master(MasterDrag { origin_y: y, origin: gain_to_position(self.master_gain()) }));
            return true;
        }
        let host = self.panel_host();
        if !self.project.lock && self.curve_exists(host, &Aim::Gain) {
            self.delete_curve(host, &Aim::Gain);
        } else {
            self.set_master_gain(1.0);
        }
        true
    }

    pub(super) fn track_send_down(&mut self, track: usize, slot: usize, y: i32, reset: bool) -> bool {
        let send = self.send_ordinal(slot);
        if reset {
            for picked in self.selection.pick_of(track) {
                if !self.project.lock && self.curve_exists(Host::Track(picked), &Aim::Send(send)) {
                    self.delete_curve(Host::Track(picked), &Aim::Send(send));
                } else {
                    self.project.tracks[picked].sends[send] = 0.0;
                }
            }
        } else {
            self.drag = Some(Drag::Send(SendDrag { send, gang: self.gang(track, y, |picked| picked.sends[send]), moved: false }));
        }
        true
    }

    pub(super) fn press_track(&mut self, index: usize, button: TrackButton) -> bool {
        match button {
            TrackButton::Delete => self.remove_track(index),
            TrackButton::Solo => {
                let group = self.selection.pick_of(index);
                if self.project.tracks[index].solo {
                    for track in group {
                        self.project.tracks[track].solo = false;
                    }
                } else {
                    for (track, state) in self.project.tracks.iter_mut().enumerate() {
                        state.solo = group.contains(&track);
                    }
                }
                true
            }
            TrackButton::Mute => {
                self.project.tracks[index].mute ^= true;
                true
            }
            TrackButton::Add => {
                self.menu = Some(Menu::Add(index));
                true
            }
        }
    }

    pub(super) fn press_append(&mut self, slot: usize) -> bool {
        let Some((fresh, _)) = ADD_ITEMS.get(slot) else {
            return false;
        };
        let at = self.project.tracks.len();
        if !self.insert_track(at, fresh(), false) {
            return false;
        }
        self.select(at);
        true
    }

    pub(super) fn strip_hover_at(&self, x: i32, y: i32) -> Option<Hover> {
        let viewport = self.track_viewport();
        for (strip, tile) in self.tiles() {
            if !self.tile_shown(tile) {
                continue;
            }
            let index = match strip {
                Strip::Track(index) => index,
                Strip::Curve(curve) => {
                    if let Some(hover) = self.curve_hover_at(curve, tile, viewport, x, y) {
                        return Some(hover);
                    }
                    continue;
                }
            };
            for (slot, (button, _)) in TRACK_BUTTONS.iter().enumerate() {
                if tile_button(tile, slot).intersect(viewport).contains(x, y) {
                    return Some(Hover::Track(index, *button));
                }
            }
            for slot in 0..SENDS_PER_BANK {
                if tile_send(tile, slot).intersect(viewport).contains(x, y) {
                    return Some(Hover::TrackSend(index, slot));
                }
            }
            if volume_inner(tile).intersect(viewport).contains(x, y) {
                return Some(Hover::Volume(index));
            }
            if self.project.tracks[index].group && fold_mark(tile).intersect(viewport).contains(x, y) {
                return Some(Hover::Fold(index));
            }
            if tile.intersect(viewport).contains(x, y) {
                return Some(Hover::Tile(index));
            }
            let lane = self.lane(tile);
            if lane.rect.intersect(viewport).contains(x, y) {
                return Some(match lane.hit_clip(&self.project.tracks[index].clips, x) {
                    Some((clip, hit)) => Hover::Clip(index, clip, hit),
                    None => Hover::Lane(index),
                });
            }
        }
        let tile = self.append_tile()?;
        (0..ADD_ITEMS.len()).find(|&slot| append_button(tile, slot).intersect(viewport).contains(x, y)).map(Hover::Append)
    }
}

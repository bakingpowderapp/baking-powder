use super::*;
use super::automation::{AUTO_LINE, Armed, auto_y, sketch_plot};
use super::region::{Covered, Region};
use crate::project::LfoShape;
use super::view::return_name;
use super::tracks::{TRACK_BUTTONS, TrackButton, TrackDrag};
use super::files::ADD_ITEMS;

pub(super) const AUTO_NODE: i32 = 9;
pub(super) const AUTO_HALO: i32 = 13;
const TRACE_STEPS: i32 = 4;
const PASTE_TICK: i32 = 8;
const PASTE_HALO: i32 = 1;
const STRIP_GAP: i32 = 2;
const STRIP_LEAST: i32 = 6;
const OPEN_GROUP_EDGE: bool = true;

fn region_rect(lane: Lane, region: &Region) -> Rect {
    let (left, right) = (lane.x_of(region.start), lane.x_of(region.end));
    Rect::new(left, lane.rect.y, right - left, lane.rect.h)
}

fn trace(list: &mut DrawList, lane: Lane, columns: std::ops::Range<i32>, travel: impl Fn(f64) -> f64) {
    for x in columns {
        let ys = (0..=TRACE_STEPS).map(|step| auto_y(lane, travel(lane.beats_at(x) + f64::from(step) / (f64::from(TRACE_STEPS) * lane.beat_w)) as f32));
        let (top, bottom) = ys.fold((i32::MAX, i32::MIN), |(top, bottom), y| (top.min(y), bottom.max(y)));
        list.fill(Rect::new(x, top, 1, bottom - top + AUTO_LINE), Tone::Ink100);
    }
}

impl App {
    pub(super) fn draw_curve_line(&mut self, host: Host, lane: Lane, armed: &Armed) {
        let rect = lane.rect;
        let (left, right) = (rect.x, rect.x + rect.w);
        if let Some(lfo) = self.lane_wave(host, armed) {
            if lfo.shape == LfoShape::Drawn {
                let selected = self.lane_sketch.node.filter(|_| self.sketch_lane.as_ref().is_some_and(|(owner, sketched)| *owner == host && sketched == armed));
                face::draw_sketch(&mut self.list, sketch_plot(rect), &lfo.points, selected, None, None);
                return;
            }
            let (curves, notes) = (&self.project.automation(host).curves, self.project.song_notes_of(host));
            let bpm = self.project.bpm;
            let share = self.project.rack(host).get(armed.aim.row_or_top()).map_or(1.0, |row| row.lane_share(&lfo.aim));
            trace(&mut self.list, lane, left..right, |at| lfo.travel(curves, &notes, at, bpm, share));
            return;
        }
        let nodes = self.armed_nodes(host, armed);
        let flat = nodes.is_empty().then(|| self.base_travel(host, armed).unwrap_or(0.0) as f32);
        let list = &mut self.list;
        if let Some(travel) = flat {
            list.fill(Rect::new(left, auto_y(lane, travel), rect.w, AUTO_LINE), Tone::Ink100);
            return;
        }
        let points: Vec<(i32, i32)> = nodes.iter().map(|node| (lane.x_of(node.tick), auto_y(lane, node.travel))).collect();
        let (first, last) = (points[0], points[points.len() - 1]);
        if first.0 > left {
            list.fill(Rect::new(left, first.1, first.0 - left, AUTO_LINE), Tone::Ink100);
        }
        if last.0 < right {
            list.fill(Rect::new(last.0, last.1, right - last.0, AUTO_LINE), Tone::Ink100);
        }
        for pair in points.windows(2) {
            let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
            if x1 - x0 <= 1 {
                list.fill(Rect::new(x0, y0.min(y1), 1, (y0 - y1).abs() + AUTO_LINE), Tone::Ink100);
                continue;
            }
            if y0 == y1 {
                list.fill(Rect::new(x0, y0, x1 - x0, AUTO_LINE), Tone::Ink100);
                continue;
            }
            trace(list, lane, x0.max(left)..x1.min(right), |at| travel_at(&nodes, at));
        }
        for &(x, y) in points.iter().filter(|(x, _)| *x >= left - AUTO_HALO && *x <= right + AUTO_HALO) {
            let centre = y + AUTO_LINE / 2;
            list.fill(Rect::new(x - AUTO_HALO / 2, centre - AUTO_HALO / 2, AUTO_HALO, AUTO_HALO), Tone::Ground);
            list.fill(Rect::new(x - AUTO_NODE / 2, centre - AUTO_NODE / 2, AUTO_NODE, AUTO_NODE), Tone::Ink100);
        }
    }

    pub(super) fn draw_tracks(&mut self) {
        let viewport = self.track_viewport();
        self.timeline.track_scroll = self.timeline.track_scroll.clamp(0, self.max_track_scroll());
        let drag = self.track_drag().filter(|drag| drag.armed);
        let carried = drag.map_or_else(Vec::new, |drag| self.carried(drag));
        let automations = self.automations();
        let tiles = self.tiles();
        self.list.clip(Some(viewport));
        for &(strip, tile) in &tiles {
            if !self.tile_shown(tile) {
                continue;
            }
            let track = match strip {
                Strip::Track(track) => track,
                Strip::Curve(curve) => {
                    let (host, armed) = automations[curve].clone();
                    self.draw_curve_lane(host, armed.clone(), tile, viewport);
                    self.draw_curve_tile(host, armed, curve, tile, viewport);
                    continue;
                }
            };
            self.draw_lane(track, tile, viewport);
            if carried.iter().any(|&(held, _)| held == track) {
                continue;
            }
            self.draw_track(track, tile, viewport);
        }
        if let Some(tile) = self.append_tile().filter(|tile| self.tile_shown(*tile)) {
            self.draw_append(tile);
        }
        self.draw_group_lines(&tiles, &carried, viewport);
        let whole = Rect::new(0, viewport.y, viewport.x + viewport.w, viewport.h);
        self.list.clip(Some(whole));
        for &(track, tile) in &carried {
            self.draw_track(track, tile, whole);
        }
        let lifted: Vec<(Strip, Rect)> = carried.iter().map(|&(track, tile)| (Strip::Track(track), tile)).collect();
        self.draw_group_lines(&lifted, &[], viewport);
        if let Some(drag) = drag {
            self.draw_drop_line(drag);
        }
        self.list.clip(None);
    }

    fn group_spans(&self, tiles: &[(Strip, Rect)], carried: &[(usize, Rect)]) -> Vec<(i32, i32)> {
        let closed = |(_, top, bottom): (usize, i32, Option<i32>)| bottom.map(|bottom| (top, bottom));
        let mut spans = Vec::new();
        let mut open: Option<(usize, i32, Option<i32>)> = None;
        let mut joined = false;
        for &(strip, tile) in tiles {
            if let Strip::Track(track) = strip {
                if carried.iter().any(|&(held, _)| held == track) {
                    joined = false;
                    continue;
                }
                joined = open.is_some_and(|(group, ..)| self.project.group_of(track) == Some(group));
                if !joined {
                    spans.extend(open.take().and_then(closed));
                    open = self.project.tracks[track].group.then_some((track, tile.y + tile.h, None));
                }
            }
            if joined && let Some((_, _, bottom)) = &mut open {
                *bottom = Some(tile.y + tile.h);
            }
        }
        spans.extend(open.and_then(closed));
        spans
    }

    fn draw_group_lines(&mut self, tiles: &[(Strip, Rect)], carried: &[(usize, Rect)], viewport: Rect) {
        let spans = self.group_spans(tiles, carried);
        let (x, w, tone) = if OPEN_GROUP_EDGE { (0, BORDER, Tone::Ground) } else { (BORDER / 4, BORDER / 2, Tone::Ink50) };
        self.list.clip(Some(Rect::new(0, viewport.y, BORDER, viewport.h)));
        for (top, bottom) in spans {
            self.list.fill(Rect::new(x, top, w, bottom - BORDER - top), tone);
        }
        self.list.clip(Some(viewport));
    }

    fn draw_drop_line(&mut self, drag: TrackDrag) {
        let spot = self.drop_spot(drag);
        let group_leads = self.dragged_tracks(drag).first().is_some_and(|&lead| self.project.tracks[lead].group);
        let tile = self.track_tile(spot.slot);
        let (column, top) = (column_w(), tile.y - BORDER / 2);
        let from = if spot.inside && !group_leads { name_x(tile, true) } else { 0 };
        self.list.fill(Rect::new(BORDER, top, column - 2 * BORDER, 2 * BORDER), Tone::Ground);
        self.list.fill(Rect::new(from, top, column - from, 2 * BORDER), Tone::Ink100);
    }

    fn draw_append(&mut self, tile: Rect) {
        self.list.fill(tile, Tone::Ground);
        self.list.frame(tile, BORDER, Tone::Ink100);
        for (slot, (_, label)) in ADD_ITEMS.iter().enumerate() {
            self.draw_control(append_button(tile, slot), CONTROL_BORDER, label, false, self.hover == Hover::Append(slot), false);
        }
    }

    pub(super) fn draw_track(&mut self, track: usize, tile: Rect, viewport: Rect) {
        self.list.fill(tile, Tone::Ground);
        self.list.frame(tile, BORDER, Tone::Ink100);
        let inner = tile_inner(tile);
        let title = tile_title(tile);
        let (group, folded) = (self.project.tracks[track].group, self.project.tracks[track].folded);
        let (x, y) = (name_x(tile, group), inner.y + HEADER_PAD);
        self.list.clip(Some(inner.intersect(viewport)));
        let selected = track == self.selection.track;
        let picked = self.selection.track_picks.contains(&track);
        if selected || picked {
            self.list.fill(title, if selected { Tone::Ink100 } else { Tone::Ink75 });
        }
        let ink = if selected || picked { Tone::Ground } else { Tone::Ink100 };
        if group {
            self.draw_mark_at(inner.x + HEADER_PAD, y + (font::GLYPH_H * TEXT_SCALE - FOLD_MARK_W) / 2, if folded { Mark::Right } else { Mark::Down }, 1, ink);
        }
        let room = font::glyphs_in(inner.x + inner.w - x, TEXT_SCALE);
        match self.rename.as_ref().filter(|rename| rename.track == track) {
            Some(rename) => {
                let shown: String = rename.text.chars().take(room).collect();
                self.list.text(x, y, &shown, ink, TEXT_SCALE);
                self.list.fill(Rect::new(x + shown.chars().count() as i32 * font::ADVANCE * TEXT_SCALE, y, TEXT_SCALE, font::GLYPH_H * TEXT_SCALE), ink);
            }
            None => {
                let label: String = self.project.tracks[track].name.chars().take(room).collect();
                self.list.text(x, y, &label, ink, TEXT_SCALE);
            }
        }
        let readout = match &self.drag {
            Some(Drag::Volume(gang)) if gang.holds(track) => Some(decibels(self.project.tracks[track].gain)),
            Some(Drag::Send(drag)) if drag.gang.holds(track) => Some(format!("{} {}%", SEND_NAMES[drag.send], (self.project.tracks[track].sends[drag.send] * 100.0).round())),
            Some(Drag::Delay(gang)) if gang.holds(track) => Some(milliseconds(self.project.tracks[track].delay_ms)),
            _ => None,
        };
        let top = title.y + title.h;
        let band = Rect::new(inner.x, top, inner.w, tile_send(tile, 0).y - top);
        if let Some(readout) = readout {
            self.list.text_centered(band, &readout, Tone::Ink100, TEXT_SCALE);
        } else if let Some(from) = self.project.tracks[track].follows.and_then(|source| self.project.tracks.get(source)) {
            let glyphs = font::glyphs_in(inner.x + inner.w - x, face::LABEL_SCALE);
            self.list.text(x, band.y + (band.h - font::GLYPH_H * face::LABEL_SCALE) / 2, &font::shortened(&from.name, glyphs), Tone::Ink50, face::LABEL_SCALE);
        }
        self.list.clip(Some(viewport));
        let mut lit = Vec::with_capacity(TRACK_BUTTONS.len());
        for (slot, (button, label)) in TRACK_BUTTONS.iter().enumerate() {
            let state = &self.project.tracks[track];
            let on = match button {
                TrackButton::Solo => state.solo,
                TrackButton::Mute => state.mute,
                TrackButton::Add => self.menu.map(Menu::root) == Some(Menu::Add(track)),
                TrackButton::Delete => false,
            };
            let hover = self.hover == Hover::Track(track, *button);
            self.draw_control(tile_button(tile, slot), CONTROL_BORDER, label, false, hover, on);
            lit.push(on);
        }
        for slot in 1..lit.len() {
            if lit[slot - 1] && lit[slot] {
                self.list.divide(tile_button(tile, slot - 1), tile_button(tile, slot), CONTROL_BORDER);
            }
        }
        for slot in 0..SENDS_PER_BANK {
            let send = self.send_ordinal(slot);
            let hover = self.hover == Hover::TrackSend(track, slot);
            let cell = tile_send(tile, slot);
            self.draw_rise(cell, CONTROL_BORDER, SEND_NAMES[send], self.automated_send(track, send), hover);
            if self.curve_exists(Host::Track(track), &Aim::Send(send)) {
                face::draw_dot(&mut self.list, cell.x + cell.w - CONTROL_BORDER - face::DOT_INSET - face::DOT, cell.y + CONTROL_BORDER + face::DOT_INSET);
            }
        }
        self.draw_volume(track, tile);
    }

    pub(super) fn draw_lane(&mut self, track: usize, tile: Rect, viewport: Rect) {
        let lane = self.lane(tile);
        let hover = self.hover;
        let picking = self.picked_notes().is_some();
        let clip_drag = self.clip_drag();
        let drag = clip_drag.filter(|drag| drag.armed);
        let resized = drag.filter(|drag| drag.resizing() && drag.track == track).map(|drag| (drag.index, self.resized(drag, &self.project.tracks[track].clips[drag.index])));
        let moving = drag.is_some_and(|drag| !drag.resizing());
        let bracketed = |index: usize| match (clip_drag, hover) {
            (Some(drag), _) => drag.resizing() && (drag.track, drag.index) == (track, index),
            (None, Hover::Clip(hovered, at, hit)) => hit != ClipHit::Body && (hovered, at) == (track, index),
            _ => false,
        };
        let preview = self.take_preview(track);
        let range = preview.as_deref().map_or_else(|| self.project.tracks[track].pitch_range(), pitch_range);
        let hidden = |clip: &Clip| moving && self.selection.clips.contains(&(track, clip.start));
        let bpm = self.project.bpm;
        let strips = if self.project.tracks[track].folded { self.project.members_of(track) } else { Vec::new() };
        let shift = drag.filter(|drag| !drag.resizing() && !strips.is_empty()).and_then(|drag| self.clip_shift(drag)).map_or(0, |(by, _)| by);
        let marker = self.selection.paste_point.filter(|(owner, _)| self.lane_of(*owner) == track);
        let held = self.selection.region.as_ref().filter(|region| region.covers.contains(&Covered::Track(track)));
        let wave_of = |clip: &Clip| clip.file.as_ref().and_then(|file| self.pool.wave(&file.name)).map(Arc::as_ref);
        let list = &mut self.list;
        list.fill(Rect::new(tile.x, tile.y, self.width, BORDER), Tone::Ink100);
        list.fill(Rect::new(tile.x, tile.y + tile.h - BORDER, self.width, BORDER), Tone::Ink100);
        list.clip(Some(lane.rect.intersect(viewport)));
        if let Some(region) = held {
            list.fill(region_rect(lane, region), Tone::Ink25);
        }
        lane.draw(list);
        let area = Rect::new(lane.rect.x, lane.rect.y + STRIP_GAP, lane.rect.w, lane.rect.h - 2 * STRIP_GAP);
        let pitch = (area.h + STRIP_GAP) / strips.len().max(1) as i32;
        for (row, member) in strips.iter().enumerate() {
            let (y, h) = if pitch >= STRIP_LEAST { (area.y + row as i32 * pitch, pitch - STRIP_GAP) } else { (area.y, area.h) };
            for clip in &self.project.tracks[*member].clips {
                let picked = self.selection.clips.contains(&(*member, clip.start));
                let moved = if picked { shift } else { 0 };
                let whole = lane.clip_rect(clip.start + moved, clip.length);
                list.fill(Rect::new(whole.x, y, whole.w, h), if picked { Tone::Ink50 } else { Tone::Ink25 });
            }
        }
        for (index, clip) in self.project.tracks[track].clips.iter().enumerate() {
            if hidden(clip) {
                continue;
            }
            let shown = match &resized {
                Some((resizing, shown)) if *resizing == index => shown,
                _ => clip,
            };
            let look = if !picking && self.selection.clips.contains(&(track, clip.start)) {
                ClipLook::Picked
            } else if matches!(hover, Hover::Clip(hovered, at, _) if (hovered, at) == (track, index)) || held.is_some_and(|region| region.holds(clip)) {
                ClipLook::Hover
            } else {
                ClipLook::Plain
            };
            lane.draw_clip(list, shown, shown.start, shown.length, look, bracketed(index), range, wave_of(shown), bpm);
        }
        if let Some(preview) = &preview {
            for clip in preview.iter().filter(|clip| !hidden(clip) && !self.project.tracks[track].clips.contains(clip)) {
                lane.draw_clip(list, clip, clip.start, clip.length, ClipLook::Plain, false, range, wave_of(clip), bpm);
            }
        }
        if let Some((_, tick)) = marker {
            let (x, top) = (lane.x_of(tick), lane.rect.y);
            list.fill(Rect::new(x - PASTE_HALO, top, grid::PLAYHEAD_W + 2 * PASTE_HALO, lane.rect.h), Tone::Ground);
            list.fill(Rect::new(x, top, grid::PLAYHEAD_W, lane.rect.h), Tone::Ink100);
            for y in [top, top + lane.rect.h - grid::PLAYHEAD_W] {
                list.fill(Rect::new(x - PASTE_TICK / 2, y, PASTE_TICK, grid::PLAYHEAD_W), Tone::Ink100);
            }
        }
        self.draw_duck(track, lane);
        self.list.clip(Some(viewport));
    }

    pub(super) fn draw_curve_lane(&mut self, host: Host, armed: Armed, tile: Rect, viewport: Rect) {
        let lane = self.lane(tile);
        let list = &mut self.list;
        list.fill(Rect::new(tile.x, tile.y, self.width, BORDER), Tone::Ink100);
        list.fill(Rect::new(tile.x, tile.y + tile.h - BORDER, self.width, BORDER), Tone::Ink100);
        list.clip(Some(lane.rect.intersect(viewport)));
        if let Some(region) = self.selection.region.as_ref().filter(|region| region.covers.contains(&Covered::Curve(host, armed.clone()))) {
            list.fill(region_rect(lane, region), Tone::Ink25);
        }
        lane.draw(list);
        self.draw_curve_line(host, lane, &armed);
        if host.track().is_none() {
            self.list.text(lane.rect.x + HEADER_PAD, lane.rect.y + HEADER_PAD, &return_name(host), Tone::Ink50, TEXT_SCALE);
        }
        self.list.clip(Some(viewport));
    }

    pub(super) fn draw_curve_tile(&mut self, host: Host, armed: Armed, lane: usize, tile: Rect, viewport: Rect) {
        let name = Rect::new(tile.x, tile.y, column_w(), tile.h);
        self.list.fill(name, Tone::Ground);
        self.list.frame(name, BORDER, Tone::Ink100);
        let inner = tile_inner(name);
        self.list.clip(Some(inner.intersect(viewport)));
        let spacing = font::GLYPH_H * TEXT_SCALE + 2;
        let (band, param) = armed.aim.words();
        let field = armed.lfo.map(|field| format!("LFO {}", field.label()));
        let lines = [Some(band), Some(param), field.as_deref()].into_iter().flatten();
        let clear = tile_clear(tile);
        let glyphs = font::glyphs_in(clear.x - inner.x - HEADER_PAD, TEXT_SCALE);
        for (line, text) in lines.enumerate() {
            self.list.text(inner.x + HEADER_PAD, inner.y + 2 + line as i32 * spacing, &font::shortened(text, glyphs), Tone::Ink100, TEXT_SCALE);
        }
        self.list.clip(Some(viewport));
        self.draw_control(clear, CONTROL_BORDER, "X", false, self.hover == Hover::Clear(lane), false);
        if armed.lfo.is_some() {
            return;
        }
        let entry = self.lane_lfo(host, &armed);
        let (ox, oy) = lfo_origin(tile);
        self.list.transform((ox, oy), 1.0);
        if self.lfo_strip == Some(lane) {
            self.lfo_face.draw_band(&mut self.list, lfo_band(&entry), &entry);
        } else {
            Face::new().draw_band(&mut self.list, lfo_band(&entry), &entry);
        }
        self.list.transform((0, 0), 1.0);
        self.list.clip(Some(viewport));
    }

    pub(super) fn draw_volume(&mut self, track: usize, tile: Rect) {
        self.list.frame(tile_volume(tile), BORDER, Tone::Ink100);
        let inner = volume_inner(tile);
        let pressed = matches!(&self.drag, Some(Drag::Volume(gang)) if gang.holds(track));
        if pressed || self.hover == Hover::Volume(track) {
            self.list.fill(inner, Tone::Ink25);
        }
        let level = self.audio.as_ref().map_or((0.0, 0.0), |audio| audio.level(track));
        self.draw_fader(inner, self.automated_gain(Host::Track(track)), level, (false, false), 0);
        if self.curve_exists(Host::Track(track), &Aim::Gain) {
            face::draw_dot(&mut self.list, inner.x + inner.w - face::DOT_INSET - face::DOT, inner.y + face::DOT_INSET);
        }
    }

    fn strip_travel(&self, host: Host, aim: &Aim) -> Option<f64> {
        let lanes = self.project.automation(host);
        let waved = lanes.lfo(aim).cloned().or_else(|| {
            let referenced = lanes.curves.iter().any(|curve| curve.lfo.is_some() && curve.aim == *aim);
            referenced.then(|| lanes.lfos.iter().find(|lfo| lfo.aim == *aim).cloned().unwrap_or_else(|| LaneLfo::blank(aim.clone())))
        });
        if let Some(lfo) = waved {
            return Some(self.lane_travel(host, &lfo, &lanes.curves, 1.0));
        }
        lanes.curve(aim).map(|curve| curve.at(self.playhead))
    }

    pub(super) fn automated_gain(&self, host: Host) -> f32 {
        self.strip_travel(host, &Aim::Gain).map_or(self.project.gain_of(host), |travel| position_to_gain(travel as f32))
    }

    pub(super) fn automated_send(&self, track: usize, send: usize) -> f32 {
        self.strip_travel(Host::Track(track), &Aim::Send(send)).map_or(self.project.tracks[track].sends[send], |travel| travel as f32)
    }
}

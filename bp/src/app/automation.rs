use super::*;
use crate::project::{LfoClock, LfoShape};

pub(super) const AUTO_GRAB: i32 = 8;
const LFO_CELLS_W: i32 = 4 * face::PITCH + face::FRAME;
const SKETCH_INSET: i32 = 6;

pub(super) const AUTO_INSET: i32 = 4;
pub(super) const AUTO_LINE: i32 = 3;

#[derive(Clone, PartialEq, Eq, Debug)]
pub(super) struct Armed {
    pub(super) aim: Aim,
    pub(super) lfo: Option<LfoField>,
}

impl Armed {
    pub(super) fn of(aim: Aim) -> Armed {
        Armed { aim, lfo: None }
    }
}

#[derive(Clone, Copy)]
pub(super) enum AutoDrag {
    Node { tick: i32, lo: i32, hi: i32, travel: f32, bend: f32, grab: i32 },
    Bend { at: f64, travel: f64, y: i32 },
}

pub(super) fn neighbors(nodes: &[Node], tick: i32) -> (i32, i32) {
    let lo = nodes.iter().map(|node| node.tick).filter(|node| *node < tick).max().map_or(0, |node| node + 1);
    let hi = nodes.iter().map(|node| node.tick).filter(|node| *node > tick).min().map_or(i32::MAX, |node| node - 1);
    (lo, hi)
}

pub(super) fn segment_around(nodes: &[Node], beat: f64) -> Option<(Node, Node)> {
    let after = nodes.partition_point(|node| beats(node.tick) <= beat);
    Some((nodes[after.checked_sub(1)?], *nodes.get(after)?))
}

pub(super) fn auto_depth(lane: Lane) -> i32 {
    (lane.rect.h - 2 * AUTO_INSET - AUTO_LINE).max(1)
}

pub(super) fn auto_travel(lane: Lane, y: i32) -> f32 {
    let top = lane.rect.y + AUTO_INSET;
    (1.0 - (y - top) as f32 / auto_depth(lane) as f32).clamp(0.0, 1.0)
}

pub(super) fn auto_y(lane: Lane, travel: f32) -> i32 {
    lane.rect.y + AUTO_INSET + ((1.0 - travel.clamp(0.0, 1.0)) * auto_depth(lane) as f32).round() as i32
}

pub(super) fn sketch_plot(rect: Rect) -> Rect {
    Rect::new(rect.x + SKETCH_INSET, rect.y + SKETCH_INSET, rect.w - 2 * SKETCH_INSET, rect.h - 2 * SKETCH_INSET)
}

impl App {
    pub(super) fn curve_exists(&self, host: Host, aim: &Aim) -> bool {
        self.project.automation(host).curve(aim).is_some()
    }

    pub(super) fn delete_curve(&mut self, host: Host, aim: &Aim) {
        self.project.automation_mut(host).curves.retain(|curve| !(curve.lfo.is_none() && curve.aim == *aim));
    }

    pub(super) fn clear_lane(&mut self, lane: usize) -> bool {
        let lanes = self.automations();
        let Some((host, armed)) = lanes.get(lane).cloned() else {
            return false;
        };
        if self.project.lock {
            return false;
        }
        let held = self.lfo_strip.and_then(|strip| lanes.get(strip).cloned());
        let automation = self.project.automation_mut(host);
        automation.curves.retain(|curve| !(curve.aim == armed.aim && armed.lfo.is_none_or(|field| curve.lfo == Some(field))));
        if armed.lfo.is_none() {
            automation.remove_lfo(&armed.aim);
        }
        self.regain_strip(held);
        true
    }

    pub(super) fn held_strip(&self) -> Option<(Host, Armed)> {
        self.lfo_strip.and_then(|strip| self.automations().get(strip).cloned())
    }

    pub(super) fn regain_strip(&mut self, held: Option<(Host, Armed)>) {
        self.lfo_strip = held.and_then(|held| self.automations().into_iter().position(|lane| lane == held));
        if self.lfo_strip.is_none() {
            self.lfo_face.close();
        }
    }

    pub(super) fn lane_lfo(&self, host: Host, armed: &Armed) -> LaneLfo {
        let lanes = self.project.automation(host);
        lanes.lfos.iter().find(|lfo| lfo.aim == armed.aim).cloned().unwrap_or_else(|| LaneLfo::blank(armed.aim.clone()))
    }

    fn lfo_curves_exist(&self, host: Host, armed: &Armed) -> bool {
        self.project.automation(host).curves.iter().any(|curve| curve.lfo.is_some() && curve.aim == armed.aim)
    }

    pub(super) fn lane_wave(&self, host: Host, armed: &Armed) -> Option<LaneLfo> {
        if armed.lfo.is_some() {
            return None;
        }
        if let Some(lfo) = self.project.automation(host).lfo(&armed.aim) {
            return Some(lfo.clone());
        }
        self.lfo_curves_exist(host, armed).then(|| self.lane_lfo(host, armed))
    }

    pub(super) fn automatable(&self, host: Host, aim: &Aim) -> bool {
        match aim {
            Aim::Gain => true,
            Aim::Send(_) => host.track().is_some(),
            Aim::Level(row) | Aim::Wet(row) => self.project.rack(host).get(*row).is_some_and(|entry| entry.mix_aim(*row).as_ref() == Some(aim)),
            Aim::Knob { row, band, name } => self.project.rack(host).get(*row).is_some_and(|entry| entry.automatable(band, name)),
        }
    }

    pub(super) fn armed_now(&self) -> Option<Armed> {
        self.armed.clone().filter(|armed| self.automatable(self.host(), &armed.aim))
    }

    pub(super) fn armed_nodes(&self, host: Host, armed: &Armed) -> Vec<Node> {
        let lanes = self.project.automation(host);
        lanes.curves.iter().find(|curve| curve.aim == armed.aim && curve.lfo == armed.lfo).map(|curve| curve.nodes.clone()).unwrap_or_default()
    }

    pub(super) fn base_travel(&self, host: Host, armed: &Armed) -> Option<f64> {
        if let Some(field) = armed.lfo {
            let entry = self.lane_lfo(host, armed);
            return Some(match field {
                LfoField::Phase => entry.phase,
                LfoField::Center => entry.centre,
                LfoField::Amount => entry.amount,
            });
        }
        match &armed.aim {
            Aim::Gain => Some(f64::from(gain_to_position(self.project.gain_of(host)))),
            Aim::Send(send) => Some(f64::from(*self.project.tracks[host.track()?].sends.get(*send)?)),
            Aim::Level(row) | Aim::Wet(row) => Some(self.project.rack(host).get(*row)?.mix()),
            Aim::Knob { row, band, name } => self.project.rack(host).get(*row)?.travel_of(band, name),
        }
    }

    pub(super) fn armed_spelling(&self, host: Host, armed: &Armed, travel: f64) -> Option<(String, &'static str)> {
        if let Some(field) = armed.lfo {
            return face::automatable(&LFO_BAND, "LFO", field.label()).map(|cell| (face::spell(cell.format, cell.param.scale.value(travel)), cell.desc));
        }
        let percent = face::spell(Format::Percent, travel);
        match &armed.aim {
            Aim::Gain => {
                let desc = match host {
                    Host::Master => "The master's volume",
                    Host::Send(_) => "The return's volume",
                    Host::Track(_) => "The track's volume",
                };
                Some((decibels(position_to_gain(travel as f32)), desc))
            }
            Aim::Send(_) => Some((percent, "How much of the track feeds the return")),
            Aim::Level(row) | Aim::Wet(row) => Some((percent, self.project.rack(host).get(*row)?.mix_desc())),
            Aim::Knob { row, band, name } => self.project.rack(host).get(*row)?.faced().spelling(band, name, travel),
        }
    }

    pub(super) fn node_down(&mut self, host: Host, armed: Armed, slot: usize, x: i32, y: i32, clicks: i32) -> bool {
        if self.project.lock {
            return false;
        }
        let lane = self.lane(self.track_tile(slot));
        let nodes = self.armed_nodes(host, &armed);
        if let Some(held) = nodes.iter().copied().filter(|node| (lane.x_of(node.tick) - x).abs() <= AUTO_GRAB).min_by_key(|node| (auto_y(lane, node.travel) - y).abs()) {
            if clicks >= 2 {
                self.project.automation_mut(host).remove_node(&armed.aim, armed.lfo, held.tick);
                return true;
            }
            let (lo, hi) = neighbors(&nodes, held.tick);
            self.drag = Some(Drag::Auto { host, armed, slot, drag: AutoDrag::Node { tick: held.tick, lo, hi, travel: held.travel, bend: held.bend, grab: 0 } });
            return true;
        }
        if self.shift && self.lane_wave(host, &armed).is_none() {
            self.drag = self.bend_down(host, &armed, lane, (x, y), clicks, &nodes).map(|drag| Drag::Auto { host, armed, slot, drag });
            return true;
        }
        let tick = lane.snapped(x).max(0);
        let travel = self.line_travel(host, &armed, &nodes, tick);
        let bend = match nodes.iter().find(|node| node.tick == tick) {
            Some(held) => held.bend,
            None => self.split_bow(host, &armed, &nodes, Node { tick, travel, bend: 0.0 }),
        };
        self.project.automation_mut(host).set_node(&armed.aim, armed.lfo, Node { tick, travel, bend });
        let (lo, hi) = neighbors(&nodes, tick);
        self.drag = Some(Drag::Auto { host, armed, slot, drag: AutoDrag::Node { tick, lo, hi, travel, bend, grab: y - auto_y(lane, travel) } });
        true
    }

    fn line_travel(&self, host: Host, armed: &Armed, nodes: &[Node], tick: i32) -> f32 {
        if nodes.is_empty() { self.base_travel(host, armed).unwrap_or(0.0) as f32 } else { travel_at(nodes, beats(tick)) as f32 }
    }

    pub(super) fn split_bow(&mut self, host: Host, armed: &Armed, nodes: &[Node], laid: Node) -> f32 {
        let Some((from, to)) = segment_around(nodes, beats(laid.tick)).filter(|(from, _)| from.bend != 0.0) else {
            return 0.0;
        };
        let span = beats(to.tick - from.tick);
        let s = beats(laid.tick - from.tick) / span;
        let was = |t: f64| travel_at(nodes, beats(from.tick) + t * span);
        let left = bend_through(f64::from(from.travel), f64::from(laid.travel), 0.5, was(s / 2.0));
        self.project.automation_mut(host).bend_node(&armed.aim, armed.lfo, from.tick, left);
        bend_through(f64::from(laid.travel), f64::from(to.travel), 0.5, was((1.0 + s) / 2.0))
    }

    pub(super) fn bend_down(&mut self, host: Host, armed: &Armed, lane: Lane, (x, y): (i32, i32), clicks: i32, nodes: &[Node]) -> Option<AutoDrag> {
        let at = lane.beats_at(x);
        let (from, _) = segment_around(nodes, at)?;
        if clicks >= 2 {
            self.project.automation_mut(host).bend_node(&armed.aim, armed.lfo, from.tick, 0.0);
            return None;
        }
        Some(AutoDrag::Bend { at, travel: travel_at(nodes, at), y })
    }

    pub(super) fn auto_moved(&mut self, x: i32, y: i32) -> bool {
        let Some(Drag::Auto { host, armed, slot, drag }) = &self.drag else {
            return false;
        };
        let (host, armed, slot, drag) = (*host, armed.clone(), *slot, *drag);
        let lane = self.lane(self.track_tile(slot));
        match drag {
            AutoDrag::Node { tick: held, lo, hi, bend, grab, .. } => {
                let travel = auto_travel(lane, y - grab);
                let snapped = lane.snapped(x).max(0);
                let tick = snapped.clamp(lo, hi);
                let lanes = self.project.automation_mut(host);
                if tick != held {
                    lanes.remove_node(&armed.aim, armed.lfo, held);
                }
                lanes.set_node(&armed.aim, armed.lfo, Node { tick, travel, bend });
                self.drag = Some(Drag::Auto { host, armed, slot, drag: AutoDrag::Node { tick, lo, hi, travel, bend, grab } });
            }
            AutoDrag::Bend { at, travel, y: pressed } => {
                let Some((from, to)) = segment_around(&self.armed_nodes(host, &armed), at) else {
                    return false;
                };
                let t = (at - beats(from.tick)) / beats(to.tick - from.tick);
                let through = travel + f64::from(pressed - y) / f64::from(auto_depth(lane));
                let bend = bend_through(f64::from(from.travel), f64::from(to.travel), t, through);
                self.project.automation_mut(host).bend_node(&armed.aim, armed.lfo, from.tick, bend);
            }
        }
        true
    }

    pub(super) fn dragged_travel(&self, host: Host, drag: AutoDrag, armed: &Armed) -> f64 {
        match drag {
            AutoDrag::Node { travel, .. } => f64::from(travel),
            AutoDrag::Bend { at, .. } => travel_at(&self.armed_nodes(host, armed), at),
        }
    }

    pub(super) fn lane_travel(&self, host: Host, lfo: &LaneLfo, curves: &[Curve], share: f64) -> f64 {
        match &self.audio {
            Some(audio) => {
                let track = host.track();
                let lane = track.filter(|_| lfo.clock == LfoClock::Note || lfo.retrigger).and_then(|track| audio.noted(track, lfo.aim.row_or_top())).unwrap_or_else(|| audio.lane());
                lfo.travel_cycles(curves, self.playhead, lfo.cycles_elapsed(lane.beats, lane.seconds), lane.seconds, share)
            }
            None => lfo.travel(curves, &self.project.song_notes_of(host), self.playhead, self.project.bpm, share),
        }
    }

    pub(super) fn automated_row(&self, host: Host, row: usize) -> Row {
        let mut shown = self.project.rack(host)[row];
        let lanes = self.project.automation(host);
        for curve in lanes.curves.iter().filter(|curve| curve.lfo.is_none()) {
            if let Aim::Knob { row: at, band, name } = &curve.aim
                && *at == row
            {
                shown.set_travel(band, name, curve.at(self.playhead));
            }
        }
        let mut lfos: Vec<LaneLfo> = lanes.lfos.iter().filter(|lfo| lfo.aim.row() == Some(row) && lfo.amount > 0.0).cloned().collect();
        for curve in lanes.curves.iter().filter(|curve| curve.aim.row() == Some(row) && curve.lfo.is_some()) {
            if !lfos.iter().any(|lfo| lfo.aim == curve.aim) {
                let stored = lanes.lfos.iter().find(|lfo| lfo.aim == curve.aim).cloned();
                lfos.push(stored.unwrap_or_else(|| LaneLfo::blank(curve.aim.clone())));
            }
        }
        for lfo in &lfos {
            if let Aim::Knob { band, name, .. } = &lfo.aim {
                let travel = self.lane_travel(host, lfo, &lanes.curves, shown.lane_share(&lfo.aim));
                shown.set_travel(band, name, travel);
            }
        }
        shown
    }

    pub(super) fn settle_lane(&mut self, host: Host, armed: &Armed, released: bool) {
        if self.project.automation(host).lfo(&armed.aim).is_some() {
            self.delete_curve(host, &armed.aim);
        }
        if !released {
            return;
        }
        let held = self.held_strip();
        self.project.automation_mut(host).settle_lfos(held.as_ref().filter(|(owner, _)| *owner == host).map(|(_, held)| &held.aim));
        self.regain_strip(held);
    }

    pub(super) fn lfo_point(&self, lane: usize, x: i32, y: i32) -> Option<(i32, i32)> {
        let slot = self.strips().into_iter().position(|strip| strip == Strip::Curve(lane))?;
        let (ox, oy) = lfo_origin(self.track_tile(slot));
        Some((x - ox, y - oy))
    }

    fn with_lfo(&mut self, host: Host, armed: &Armed, act: impl FnOnce(&mut Face, &mut Sketch, &mut LaneLfo) -> bool) -> bool {
        if self.project.lock {
            let mut scratch = self.lane_lfo(host, armed);
            return act(&mut self.lfo_face, &mut self.lane_sketch, &mut scratch);
        }
        act(&mut self.lfo_face, &mut self.lane_sketch, self.project.automation_mut(host).lfo_mut(&armed.aim))
    }

    pub(super) fn lfo_down(&mut self, lane: usize, x: i32, y: i32) -> bool {
        let Some((host, armed)) = self.automations().get(lane).filter(|(_, armed)| armed.lfo.is_none()).cloned() else {
            return false;
        };
        let Some((lx, ly)) = self.lfo_point(lane, x, y) else {
            return false;
        };
        let bpm = self.project.bpm;
        self.lfo_strip = Some(lane);
        let changed = self.with_lfo(host, &armed, |cells, _, lfo| cells.down(lx, ly, face::NO_READOUT, lfo_band(lfo), lfo, bpm));
        if !self.project.lock {
            self.settle_lane(host, &armed, !self.lfo_face.dragging() && !self.lfo_face.panel_open());
        }
        changed
    }

    pub(super) fn lfo_moved(&mut self, x: i32, y: i32) -> bool {
        let Some(lane) = self.lfo_strip else {
            return false;
        };
        let Some((host, armed)) = self.automations().get(lane).filter(|(_, armed)| armed.lfo.is_none()).cloned() else {
            return false;
        };
        let Some((lx, ly)) = self.lfo_point(lane, x, y) else {
            return false;
        };
        let changed = self.with_lfo(host, &armed, |cells, _, lfo| cells.moved(lx, ly, face::NO_READOUT, lfo_band(lfo), lfo));
        if !self.project.lock {
            self.settle_lane(host, &armed, false);
        }
        changed
    }

    pub(super) fn lfo_up(&mut self, x: i32, y: i32) -> bool {
        let Some(lane) = self.lfo_strip.filter(|_| self.lfo_face.dragging() || self.lfo_face.panel_open()) else {
            return false;
        };
        let Some((host, armed)) = self.automations().get(lane).filter(|(_, armed)| armed.lfo.is_none()).cloned() else {
            self.lfo_face.close();
            self.lfo_strip = None;
            return false;
        };
        let Some((lx, ly)) = self.lfo_point(lane, x, y) else {
            return false;
        };
        let changed = self.with_lfo(host, &armed, |cells, _, lfo| cells.up(lx, ly, face::NO_READOUT, lfo_band(lfo), lfo));
        if !self.project.lock {
            self.settle_lane(host, &armed, true);
        }
        changed
    }

    pub(super) fn lfo_hover(&mut self, x: i32, y: i32) -> bool {
        if self.lfo_face.dragging() || self.lfo_face.panel_open() {
            let Some(row) = self.lfo_strip else {
                return false;
            };
            let point = self.lfo_point(row, x, y);
            return self.lfo_face.hover(point, face::NO_READOUT, &LFO_BAND);
        }
        match self.hover {
            Hover::Lfo(row) => {
                self.lfo_strip = Some(row);
                self.lfo_face.hover(self.lfo_point(row, x, y), face::NO_READOUT, &LFO_BAND)
            }
            _ => self.lfo_face.hover(None, face::NO_READOUT, &LFO_BAND),
        }
    }

    pub(super) fn arm_lfo(&mut self, lane: usize, x: i32, y: i32) -> bool {
        let Some((host, parent)) = self.automations().get(lane).filter(|(_, armed)| armed.lfo.is_none()).cloned() else {
            return false;
        };
        let Some((lx, ly)) = self.lfo_point(lane, x, y) else {
            return false;
        };
        let entry = self.lane_lfo(host, &parent);
        let armed = face::value_cell_at(&LFO_BAND, face::NO_READOUT, lx, ly, self.lfo_face.view())
            .filter(|cell| !(cell.refuses)(&entry))
            .and_then(|cell| LfoField::ALL.iter().find(|field| field.label() == cell.param.name))
            .map(|field| Armed { lfo: Some(*field), ..parent });
        match armed {
            Some(armed) => {
                self.visit(host);
                self.toggle_aim(armed)
            }
            None => false,
        }
    }

    fn visit(&mut self, host: Host) {
        if host != self.host()
            && let Some(track) = host.track()
        {
            self.select(track);
        }
    }

    pub(super) fn arm_at(&mut self, x: i32, y: i32) -> bool {
        let Some((host, row)) = self.shown() else {
            return false;
        };
        let armed = self.project.rack(host)[row].faced().armable_at(x, y, self.face.view()).map(|(band, param)| Armed::of(Aim::knob(row, band, param)));
        match armed {
            Some(armed) => self.toggle_aim(armed),
            None => false,
        }
    }

    pub(super) fn arm_rack(&mut self, x: i32, y: i32) -> bool {
        let host = self.host();
        let (rx, ry) = self.rack_point(x, y);
        let rack = self.project.rack(host);
        match self.rack.name_at(rx, ry, rack, self.offer()) {
            Some(row) if let Some(mixed) = rack[row].mix_aim(row) => self.toggle_aim(Armed::of(mixed)),
            _ => false,
        }
    }

    pub(super) fn arm_volume(&mut self, track: usize) -> bool {
        self.arm_strip(track, Armed::of(Aim::Gain))
    }

    pub(super) fn arm_send(&mut self, track: usize, slot: usize) -> bool {
        self.arm_strip(track, Armed::of(Aim::Send(self.send_ordinal(slot))))
    }

    fn arm_strip(&mut self, track: usize, armed: Armed) -> bool {
        if self.view.auto != Auto::Off && self.monitored.is_none() && track == self.selection.track && self.armed.as_ref() == Some(&armed) {
            self.show_auto(Auto::Off);
            return true;
        }
        if track != self.selection.track || self.monitored.is_some() {
            self.select(track);
        }
        self.arm(armed);
        self.reveal_armed();
        true
    }

    pub(super) fn arm_return(&mut self) -> bool {
        let host = self.panel_host();
        if self.monitored != Some(host) {
            self.select_host(host);
        }
        self.toggle_aim(Armed::of(Aim::Gain))
    }

    pub(super) fn toggle_aim(&mut self, armed: Armed) -> bool {
        if self.view.auto != Auto::Off && self.armed.as_ref() == Some(&armed) {
            self.show_auto(Auto::Off);
            return true;
        }
        self.arm(armed);
        self.reveal_armed();
        true
    }

    pub(super) fn arm(&mut self, armed: Armed) {
        let held = self.held_strip();
        self.armed = Some(armed);
        self.regain_strip(held);
    }

    pub(super) fn reveal_armed(&mut self) {
        match self.view.auto {
            Auto::Off => self.show_auto(Auto::Track),
            Auto::Track => self.timeline.track_scroll = 0,
            Auto::All => self.scroll_to_selected(),
        }
    }

    pub(super) fn show_auto(&mut self, auto: Auto) {
        self.view.auto = auto;
        self.lfo_face.close();
        self.lfo_strip = None;
        self.scroll_to_selected();
    }

    pub(super) fn scroll_to_selected(&mut self) {
        self.timeline.track_scroll = 0;
        let top = self.track_tile(self.track_slot(self.selection.track)).y - RAIL_H;
        self.timeline.track_scroll = top.clamp(0, self.max_track_scroll());
    }

    fn keyed_armed(&self, host: Host, aim: &Aim) -> Option<(usize, Armed)> {
        let order = match aim {
            Aim::Gain => 0,
            Aim::Send(send) => *send,
            Aim::Level(row) | Aim::Wet(row) => self.project.rack(host).get(*row).filter(|entry| entry.mix_aim(*row).as_ref() == Some(aim)).map(|_| 0)?,
            Aim::Knob { row, band, name } => self.project.rack(host).get(*row)?.faced().automatable_of(band, name)?.0 + 1,
        };
        Some((order, Armed::of(aim.clone())))
    }

    fn curve_armed(&self, host: Host, curve: &Curve) -> Option<(usize, usize, Armed)> {
        let (order, parent) = self.keyed_armed(host, &curve.aim)?;
        let Some(field) = curve.lfo else {
            return Some((order, 0, parent));
        };
        let sub = face::index_of(&LFO_BAND, "LFO", field.label())? + 1;
        Some((order, sub, Armed { lfo: Some(field), ..parent }))
    }

    fn lane_hosts(&self) -> Vec<Host> {
        match self.view.auto {
            Auto::Off => Vec::new(),
            Auto::Track => vec![self.host()],
            Auto::All => self.visible_tracks().into_iter().map(|track| self.lane_host(track)).collect(),
        }
    }

    pub(super) fn automations(&self) -> Vec<(Host, Armed)> {
        self.lane_hosts().into_iter().flat_map(|host| self.automations_of(host).into_iter().map(move |armed| (host, armed))).collect()
    }

    fn automations_of(&self, host: Host) -> Vec<Armed> {
        let lanes = self.project.automation(host);
        let mut found: Vec<(usize, usize, Armed)> = Vec::new();
        let mut note = |order: usize, sub: usize, armed: Armed| {
            if !found.iter().any(|(.., seen)| *seen == armed) {
                found.push((order, sub, armed));
            }
        };
        for curve in &lanes.curves {
            if let Some((order, sub, armed)) = self.curve_armed(host, curve) {
                if sub > 0 {
                    note(order, 0, Armed { lfo: None, ..armed.clone() });
                }
                note(order, sub, armed);
            }
        }
        for lfo in &lanes.lfos {
            if let Some((order, armed)) = self.keyed_armed(host, &lfo.aim) {
                note(order, 0, armed);
            }
        }
        let current = host == self.host();
        let shown = self.shown().filter(|_| current).map(|(_, row)| row);
        found.sort_by_key(|(order, sub, armed)| (armed.aim != Aim::Gain, !matches!(armed.aim, Aim::Send(_)), Some(armed.aim.row_or_top()) != shown, armed.aim.row_or_top(), *order, *sub));
        let mut lanes: Vec<Armed> = found.into_iter().map(|(.., armed)| armed).collect();
        if let Some(armed) = self.armed_now().filter(|_| current) {
            lanes.retain(|lane| *lane != armed);
            let parent = Armed { lfo: None, ..armed.clone() };
            let child = armed.lfo.is_some();
            lanes.insert(0, armed);
            if child && !lanes.contains(&parent) {
                lanes.insert(1, parent);
            }
        }
        lanes
    }

    pub(super) fn curve_down(&mut self, curve: usize, x: i32, y: i32, clicks: i32) -> bool {
        let Some((host, armed)) = self.automations().get(curve).cloned() else {
            return false;
        };
        let Some(slot) = self.strips().into_iter().position(|strip| strip == Strip::Curve(curve)) else {
            return false;
        };
        if self.lane_wave(host, &armed).is_some_and(|lfo| lfo.shape == LfoShape::Drawn) {
            return self.sketch_down(host, armed, slot, x, y, clicks);
        }
        if clicks < 2 && !self.shift && self.off_line(host, &armed, self.lane(self.track_tile(slot)), x, y) {
            self.roll.unpick();
            let had = !self.selection.clips.is_empty() || self.selection.region.is_some();
            self.unpick_elsewhere();
            self.selection.clips.clear();
            self.sweep_region(x, y);
            return had;
        }
        self.node_down(host, armed, slot, x, y, clicks)
    }

    fn off_line(&self, host: Host, armed: &Armed, lane: Lane, x: i32, y: i32) -> bool {
        if self.lane_wave(host, armed).is_some() {
            return false;
        }
        let nodes = self.armed_nodes(host, armed);
        if nodes.iter().any(|node| (lane.x_of(node.tick) - x).abs() <= AUTO_GRAB) {
            return false;
        }
        let base = self.base_travel(host, armed).unwrap_or(0.0) as f32;
        let y_at = |px: i32| auto_y(lane, if nodes.is_empty() { base } else { travel_at(&nodes, lane.beats_at(px)) as f32 });
        (x - AUTO_GRAB..=x + AUTO_GRAB).all(|px| {
            let (a, b) = (y_at(px), y_at(px + 1));
            y + AUTO_GRAB < a.min(b) || y - AUTO_GRAB >= a.max(b) + AUTO_LINE
        })
    }

    fn sketch_rect(&self, host: Host, armed: &Armed) -> Option<Rect> {
        let lane = self.automations().into_iter().position(|(owner, lane)| owner == host && lane == *armed)?;
        let slot = self.strips().into_iter().position(|strip| strip == Strip::Curve(lane))?;
        Some(sketch_plot(self.lane(self.track_tile(slot)).rect))
    }

    fn sketch_down(&mut self, host: Host, armed: Armed, slot: usize, x: i32, y: i32, clicks: i32) -> bool {
        let plot = sketch_plot(self.lane(self.track_tile(slot)).rect);
        let shift = self.shift;
        self.sketch_lane = Some((host, armed.clone()));
        self.with_lfo(host, &armed, |_, sketch, lfo| sketch.down(x, y, plot, &mut lfo.points, shift, clicks))
    }

    pub(super) fn sketch_moved(&mut self, x: i32, y: i32) -> bool {
        let Some((host, armed)) = self.sketch_lane.clone().filter(|_| self.lane_sketch.dragging()) else {
            return false;
        };
        let Some(plot) = self.sketch_rect(host, &armed) else {
            return false;
        };
        self.with_lfo(host, &armed, |_, sketch, lfo| sketch.moved(x, y, plot, &mut lfo.points))
    }

    pub(super) fn sketch_up(&mut self) -> bool {
        let Some((host, armed)) = self.sketch_lane.clone().filter(|_| self.lane_sketch.dragging()) else {
            return false;
        };
        let changed = self.with_lfo(host, &armed, |_, sketch, lfo| sketch.up(&mut lfo.points));
        if !self.project.lock {
            self.settle_lane(host, &armed, true);
        }
        changed
    }

    pub(super) fn curve_hover_at(&self, curve: usize, tile: Rect, viewport: Rect, x: i32, y: i32) -> Option<Hover> {
        if tile_clear(tile).intersect(viewport).contains(x, y) {
            return Some(Hover::Clear(curve));
        }
        let direct = self.automations().get(curve).is_some_and(|(_, armed)| armed.lfo.is_none());
        let (ox, oy) = lfo_origin(tile);
        let cells = Rect::new(ox, oy, LFO_CELLS_W, LFO_CELLS_H);
        if direct && (cells.intersect(viewport).contains(x, y) || (self.lfo_strip == Some(curve) && self.lfo_face.panel_open())) {
            return Some(Hover::Lfo(curve));
        }
        self.lane(tile).rect.intersect(viewport).contains(x, y).then_some(Hover::Curve(curve))
    }
}

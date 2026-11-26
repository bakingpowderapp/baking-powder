use super::*;
use super::automation::Armed;

#[derive(Clone, PartialEq, Default)]
pub(super) struct Generator {
    pub(super) core: Option<CoreSolo>,
    pub(super) open: Option<usize>,
    pub(super) armed: Option<Armed>,
}

impl Generator {
    pub(super) fn to_json(&self) -> Json {
        let mut fields = Fields::new();
        if let Some(solo) = self.core {
            fields.push("core", Json::Array(vec![Json::Number(solo.row as f64), Json::Bool(solo.effects)]));
        }
        if let Some(row) = self.open {
            fields.push("open", Json::Number(row as f64));
        }
        fields.into_object().unwrap_or(Json::Object(Vec::new()))
    }

    pub(super) fn from_json(json: &Json) -> Generator {
        let json = Some(json);
        Generator { core: core_solo_from_json(items(json, "core")), open: index(json, "open", RACK_ROWS), armed: None }
    }
}

fn core_solo_from_json(parts: &[Json]) -> Option<CoreSolo> {
    let [row, effects, ..] = parts else {
        return None;
    };
    Some(CoreSolo { row: row.as_index(RACK_ROWS)?, effects: effects.as_bool()? })
}

impl Row {
    pub(super) fn faced(&self) -> &dyn Faced {
        match self {
            Row::Midi(midi) => midi,
            Row::Core(layer) => &layer.patch,
            Row::Effect(slot) => slot,
            Row::Bus(bus) => &bus.face,
        }
    }

    pub(super) fn faced_mut(&mut self) -> &mut dyn Faced {
        match self {
            Row::Midi(midi) => midi,
            Row::Core(layer) => &mut layer.patch,
            Row::Effect(slot) => slot,
            Row::Bus(bus) => &mut bus.face,
        }
    }
}

impl App {
    pub(super) fn face_hover(&mut self, x: i32, y: i32) -> bool {
        let point = (self.hover == Hover::Face).then(|| self.face_point(x, y));
        let Some((host, row)) = self.shown() else {
            return self.face.hover(point, self.readout_rows(), &face::EMPTY);
        };
        self.project.rack(host)[row].faced().hover(&mut self.face, point)
    }

    pub(super) fn rack_hover(&mut self, x: i32, y: i32) -> bool {
        let point = (self.hover == Hover::Rack).then(|| self.rack_point(x, y));
        let (host, offer) = (self.host(), self.offer());
        self.rack.hover(point, self.project.rack(host), offer)
    }

    pub(super) fn send_landing(&self, x: i32, y: i32) -> Option<usize> {
        let held = self.rack.held()?;
        if self.project.rack(self.host())[held].takes_notes() {
            return None;
        }
        (0..SENDS_PER_BANK).find(|&slot| tile_inner(self.send_cell(slot)).contains(x, y))
    }

    pub(super) fn lanes_hover(&mut self, x: i32, y: i32) -> bool {
        let point = (self.hover == Hover::Lanes).then(|| self.lanes_point(x, y));
        self.lanes.hover(point)
    }

    pub(super) fn palette_hover(&mut self, x: i32, y: i32) -> bool {
        (0..DECKS).fold(false, |changed, deck| {
            let point = (self.hover == Hover::Palette(deck)).then(|| self.palette_point(deck, x, y));
            self.palettes[deck].hover(point) | changed
        })
    }

    pub(super) fn lanes_down(&mut self, x: i32, y: i32) -> bool {
        let (lx, ly) = self.lanes_point(x, y);
        let shift = self.shift;
        self.edit_arp(|lanes, arp| lanes.down(lx, ly, shift, arp))
    }

    pub(super) fn lanes_moved(&mut self, x: i32, y: i32) -> bool {
        self.edit_pattern(|lanes, pattern| lanes.moved(x, y, pattern))
    }

    pub(super) fn face_down(&mut self, x: i32, y: i32, clicks: i32) -> bool {
        let changed = self.face_pressed(x, y, clicks);
        if changed && self.face.picking() {
            self.rack.unpick();
            self.wave_face.unpick();
            self.unpick_span();
        }
        changed
    }

    fn face_pressed(&mut self, x: i32, y: i32, clicks: i32) -> bool {
        let Some((host, row)) = self.shown() else {
            return false;
        };
        let inner = face::interior(face::readout_rect(self.readout_rows()));
        let shown_tv = self.tv_shown().filter(|_| inner.contains(x, y));
        let graph = shown_tv.and_then(|shown| match shown {
            Shown::Envelope(which, _) => Some(which),
            Shown::Lfo(_) | Shown::Wave(..) => None,
        });
        if let Some(shown) = shown_tv.filter(|_| !self.face.panel_open()) {
            let shift = self.shift;
            if let Row::Core(layer) = &mut self.project.rack_mut(host)[row]
                && self.tv.down_sketch(x, y, inner, &mut layer.patch, shown, shift, clicks)
            {
                return true;
            }
        }
        if self.shift && !self.face.panel_open() {
            let index = self.project.rack(host)[row].faced().index_at(x, y, self.face.view());
            return match (index, graph) {
                (Some(index), _) => self.reset_cell(host, row, index),
                (None, Some(which)) => self.reset_stage(host, row, which, x, y),
                (None, None) => self.picture_pressed(host, row, x, y, clicks),
            };
        }
        let (bpm, picture, shift) = (self.project.bpm, self.picture_inner(), self.shift);
        let entry = &mut self.project.rack_mut(host)[row];
        if let (Row::Core(layer), Some(which)) = (&*entry, graph) {
            return self.tv.down(x, y, inner, &layer.patch, which);
        }
        if entry.faced_mut().down(&mut self.face, x, y, bpm) {
            if let Row::Effect(slot) = &*entry {
                slot.follow_face(&self.face, &mut self.picture);
            }
            if let Some((band, param)) = self.face.dragged_cell().and_then(|cell| entry.faced().armable(cell)) {
                self.arm(Armed::of(Aim::knob(row, band, param)));
            }
            match self.face.take_act() {
                Some(Act::Flatten) => self.flatten(host, row),
                Some(Act::Match) => self.match_all(host, row),
                None => {}
            }
            return true;
        }
        let Row::Effect(slot) = entry else {
            return false;
        };
        picture.contains(x, y) && slot.press_picture(&mut self.picture, &mut self.face, x, y, picture, shift, clicks)
    }

    fn picture_pressed(&mut self, host: Host, row: usize, x: i32, y: i32, clicks: i32) -> bool {
        let inner = self.picture_inner();
        if !inner.contains(x, y) {
            return false;
        }
        let shift = self.shift;
        match &mut self.project.rack_mut(host)[row] {
            Row::Effect(slot) => slot.press_picture(&mut self.picture, &mut self.face, x, y, inner, shift, clicks),
            Row::Midi(_) | Row::Core(_) | Row::Bus(_) => false,
        }
    }

    pub(super) fn picture_right(&mut self, x: i32, y: i32) -> bool {
        let Some((host, row)) = self.shown() else {
            return false;
        };
        let inner = self.picture_inner();
        if !inner.contains(x, y) {
            return false;
        }
        match &mut self.project.rack_mut(host)[row] {
            Row::Effect(slot) => slot.right_picture(&mut self.picture, &mut self.face, x, y, inner),
            Row::Midi(_) | Row::Core(_) | Row::Bus(_) => false,
        }
    }

    pub(super) fn reset_cell(&mut self, host: Host, row: usize, index: usize) -> bool {
        let Some((band, param)) = self.project.rack_mut(host)[row].faced_mut().restore(index) else {
            return false;
        };
        if !self.project.lock {
            let aim = Aim::knob(row, band, param);
            let automation = self.project.automation_mut(host);
            automation.curves.retain(|curve| curve.aim != aim);
            automation.remove_lfo(&aim);
        }
        true
    }

    fn reset_stage(&mut self, host: Host, row: usize, which: Which, x: i32, y: i32) -> bool {
        let inner = face::interior(face::readout_rect(self.readout_rows()));
        let Row::Core(layer) = &mut self.project.rack_mut(host)[row] else {
            return false;
        };
        let Some(stage) = Tv::stage_at(x, y, inner, &layer.patch, which) else {
            return false;
        };
        if stage != Stage::Sustain {
            tv::straighten(&mut layer.patch, which, stage);
            return true;
        }
        tv::stage_cell(which, stage).is_some_and(|index| self.reset_cell(host, row, index))
    }

    pub(super) fn tv_shown(&self) -> Option<Shown> {
        if let Some((which, stage)) = self.tv.dragged() {
            return Some(Shown::Envelope(which, stage));
        }
        if let Some(shown) = self.tv.sketched() {
            return Some(shown);
        }
        let (host, row) = self.shown()?;
        if !self.project.rack(host)[row].is_core() {
            return None;
        }
        let index = self.face.shown_cell().or_else(|| self.face.picked())?;
        tv::shown_of(index)
    }

    pub(super) fn tv_hover(&mut self, x: i32, y: i32) -> bool {
        let point = (self.hover == Hover::Face).then(|| self.face_point(x, y));
        let inner = face::interior(face::readout_rect(self.readout_rows()));
        let shown = self.tv_shown().zip(self.shown());
        let graphed = shown.and_then(|(shown, (host, row))| match shown {
            Shown::Envelope(which, _) => Some((which, &self.project.rack(host)[row])),
            Shown::Lfo(_) | Shown::Wave(..) => None,
        });
        match graphed {
            Some((which, Row::Core(layer))) => self.tv.hover(point, inner, Some((which, &layer.patch))),
            _ => self.tv.hover(point, inner, None),
        }
    }

    pub(super) fn tv_up(&mut self) -> bool {
        if !self.tv.dragging() {
            return false;
        }
        match self.shown() {
            Some((host, row)) => match &mut self.project.rack_mut(host)[row] {
                Row::Core(layer) => self.tv.up(Some(&mut layer.patch)),
                Row::Midi(_) | Row::Effect(_) | Row::Bus(_) => self.tv.up(None),
            },
            None => self.tv.up(None),
        }
    }

    pub(super) fn tv_moved(&mut self, x: i32, y: i32) -> bool {
        let Some((host, row)) = self.shown() else {
            return false;
        };
        let inner = face::interior(face::readout_rect(self.readout_rows()));
        match &mut self.project.rack_mut(host)[row] {
            Row::Core(layer) => self.tv.moved(x, y, inner, &mut layer.patch),
            Row::Midi(_) | Row::Effect(_) | Row::Bus(_) => false,
        }
    }

    pub(super) fn face_moved(&mut self, x: i32, y: i32) -> bool {
        let Some((host, row)) = self.shown() else {
            return false;
        };
        self.project.rack_mut(host)[row].faced_mut().moved(&mut self.face, x, y)
    }

    pub(super) fn face_wheel(&mut self, x: i32, y: i32, dy: i32) -> bool {
        let Some((host, row)) = self.shown() else {
            return false;
        };
        let inner = self.picture_inner();
        if self.project.rack_mut(host)[row].faced_mut().wheel(&mut self.face, x, y, dy) {
            return true;
        }
        match &mut self.project.rack_mut(host)[row] {
            Row::Effect(slot) => inner.contains(x, y) && self.picture.wheel(dy, slot),
            Row::Midi(_) | Row::Core(_) | Row::Bus(_) => false,
        }
    }

    pub(super) fn picture_up(&mut self) -> bool {
        match self.shown().map(|(host, row)| &mut self.project.rack_mut(host)[row]) {
            Some(Row::Effect(slot)) => slot.release_picture(&mut self.picture, &mut self.face),
            _ => self.picture.up(None),
        }
    }

    pub(super) fn face_up(&mut self, x: i32, y: i32) -> bool {
        let Some((host, row)) = self.shown() else {
            return false;
        };
        self.project.rack_mut(host)[row].faced_mut().up(&mut self.face, x, y)
    }

    pub(super) fn rack_down(&mut self, x: i32, y: i32, clicks: i32) -> bool {
        let (rx, ry) = self.rack_point(x, y);
        let (host, offer) = (self.host(), self.offer());
        let (changed, action) = self.rack.down(rx, ry, clicks, self.shift, offer, self.project.rack_mut(host));
        if let Some(action) = action {
            self.rack_action(action);
        }
        changed || matches!(action, Some(Action::Restore(_)))
    }

    pub(super) fn rack_right(&mut self, x: i32, y: i32) -> bool {
        self.arm_rack(x, y)
    }

    pub(super) fn search_rack(&mut self) -> bool {
        if !self.core_shown() {
            return false;
        }
        self.rack.search(self.project.rack(self.host()))
    }

    pub(super) fn rack_key(&mut self, key: Key, shift: bool, command: bool) -> bool {
        let offer = self.offer();
        let (changed, action) = self.rack.type_key(key, shift, command, offer);
        if let Some(action) = action {
            self.rack_action(action);
        }
        changed
    }

    pub(super) fn picture_moved(&mut self, x: i32, y: i32) -> bool {
        let (fx, fy) = self.face_point(x, y);
        let inner = self.picture_inner();
        let (command, shift) = (self.command, self.shift);
        let Some((host, row)) = self.shown() else {
            return false;
        };
        match &mut self.project.rack_mut(host)[row] {
            Row::Effect(slot) => self.picture.moved(fx, fy, inner, slot, command, shift),
            Row::Midi(_) | Row::Core(_) | Row::Bus(_) => false,
        }
    }

    pub(super) fn rack_moved(&mut self, x: i32, y: i32) -> bool {
        let (rx, ry) = self.rack_point(x, y);
        let (host, offer) = (self.host(), self.offer());
        let landing = self.send_landing(x, y);
        let changed = self.rack.moved(rx, ry, offer, self.project.rack_mut(host), landing.is_some());
        changed | self.set_hover(landing.map_or(Hover::Rack, |slot| Hover::Button(Button::Send(slot))))
    }

    pub(super) fn stage_hover_at(&self, x: i32, y: i32) -> Option<Hover> {
        if self.browser_square().contains(x, y) {
            return Some(Hover::Button(Button::Show(Aside::Browser)));
        }
        if self.stock_offered() && self.stock_square().contains(x, y) {
            return Some(Hover::Button(Button::Stock));
        }
        if self.save_cell().contains(x, y) {
            return Some(if self.shown().is_some() { Hover::Button(Button::Save) } else { Hover::Nothing });
        }
        if self.load_cell().contains(x, y) {
            return Some(Hover::Load);
        }
        if self.spectrum_square().contains(x, y) {
            return Some(Hover::Button(Button::Show(Aside::Spectrum)));
        }
        if self.wave_offered() && self.wave_square().contains(x, y) {
            return Some(Hover::Button(Button::Show(Aside::Wave)));
        }
        if let Some((_, aside)) = self.own_button()
            && self.own_square().contains(x, y)
        {
            return Some(Hover::Button(Button::Show(aside)));
        }
        if self.face_rect().contains(x, y) {
            return Some(Hover::Face);
        }
        if self.rack_rect().contains(x, y) {
            return Some(Hover::Rack);
        }
        if self.over_big(x, y) {
            return Some(Hover::Face);
        }
        let aside = tile_inner(self.aside_cell());
        if self.view.aside == Aside::Spectrum && aside.contains(x, y) {
            return Some(Hover::Hertz(x - aside.x));
        }
        if self.view.aside == Aside::Browser && self.browser_rect().contains(x, y) {
            return Some(Hover::Browser);
        }
        if self.lanes_shown() && self.lanes_rect().contains(x, y) {
            return Some(Hover::Lanes);
        }
        if let Some(hover) = self.wave_hover_at(x, y) {
            return Some(hover);
        }
        if tile_inner(self.master_name_cell()).contains(x, y) {
            return Some(Hover::Button(Button::MasterName));
        }
        if tile_inner(self.master_volume()).contains(x, y) {
            return Some(Hover::Master);
        }
        for slot in 0..SENDS_PER_BANK {
            if tile_inner(self.send_cell(slot)).contains(x, y) {
                return Some(Hover::Button(Button::Send(slot)));
            }
        }
        if tile_inner(self.cycle_cell()).contains(x, y) {
            return Some(Hover::Button(Button::Cycle));
        }
        None
    }
}

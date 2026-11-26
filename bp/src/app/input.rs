use super::*;
use super::clips::ChordDrag;
use super::files::{ADD_ITEMS, EXPORT_ITEMS, FILE_ITEMS};
use super::presets::{SavePanel, StockPanel};
use super::transport::Latch;

const FEEDBACK_URL: &str = "https://bakingpowder.app/feedback";
const WHEEL_REST: f32 = 0.5;

impl App {
    pub fn event(&mut self, event: Event) -> bool {
        self.face.name_list(self.track_names(), self.withheld_name());
        self.face.asleep(Act::Flatten, self.flat_refusal());
        if self.small() && matches!(event, Event::Move { .. } | Event::Down { .. } | Event::RightDown { .. } | Event::MiddleDown { .. } | Event::Up { .. } | Event::Scroll { .. }) {
            return false;
        }
        if !matches!(event, Event::Move { .. } | Event::Leave | Event::Scroll { .. } | Event::Key { repeat: true, .. }) {
            self.gesture = None;
        }
        let noticed = matches!(event, Event::Down { .. } | Event::RightDown { .. } | Event::MiddleDown { .. } | Event::Key { .. }) && self.notice.take().is_some();
        let hovering = matches!(event, Event::Move { .. } | Event::Leave) && !self.dragging();
        let zoom = self.zoomed();
        let changed = match event {
            Event::Move { x, y } => self.moved(x, y),
            Event::Leave => self.unhover(),
            Event::Down { x, y, clicks } => self.press(x, y, clicks),
            Event::RightDown { x, y } => self.right_press(x, y),
            Event::MiddleDown { x, y } => self.middle_press(x, y),
            Event::Up { x, y } => self.release(x, y),
            Event::Scroll { x, y, dx, dy } => self.scroll(x, y, dx, dy),
            Event::Modifiers { shift, command } => {
                self.command = command;
                self.set_shift(shift)
            }
            Event::Key { key, shift, command, repeat } => self.key(key, shift, command, repeat),
            Event::KeyUp { key } => self.key_up(key),
            Event::Blur => self.blur(),
            Event::Focus => {
                self.drop_glances();
                false
            }
            Event::Quit => {
                self.print_take();
                self.finish_typing()
            }
        };
        if self.zoomed() != zoom && !self.dragging() {
            self.unhover();
        }
        self.peek_hovered();
        match event {
            Event::Up { .. } => self.finish_edit(),
            Event::Key { .. } => self.finish_step(Some(f32::INFINITY)),
            _ => {
                self.settle_roll();
                if matches!(event, Event::Blur | Event::Quit) {
                    self.remember();
                }
                if hovering {
                    self.sync_notes();
                } else {
                    self.sync_audio();
                }
            }
        }
        noticed | changed
    }

    fn unhover(&mut self) -> bool {
        self.hover_as(Hover::Nothing, -1, -1)
    }

    pub(super) fn rehover(&mut self, x: i32, y: i32) -> bool {
        self.hover_as(self.hover_at(x, y), x, y)
    }

    fn hover_as(&mut self, hover: Hover, x: i32, y: i32) -> bool {
        self.set_hover(hover) | self.face_hover(x, y) | self.tv_hover(x, y) | self.rack_hover(x, y) | self.lfo_hover(x, y) | self.wave_hover(x, y) | self.roll_hover(x, y) | self.ruler_hover(x) | self.bar_hover(x) | self.browser_hover(x, y) | self.lanes_hover(x, y) | self.palette_hover(x, y)
    }

    fn finish_typing(&mut self) -> bool {
        self.face.leave() | self.finish_rename() | self.finish_port() | self.browser_finish()
    }

    pub(super) fn finish_edit(&mut self) {
        self.finish_step(None);
    }

    fn finish_step(&mut self, gesture: Option<f32>) {
        self.project.seed();
        self.project.unloop_gates();
        self.settle_stock();
        self.settle_roll();
        match gesture {
            Some(rest) => self.commit_gesture(rest),
            None => self.commit(),
        }
        self.sync_audio();
        self.remember();
    }

    pub(super) fn scroll(&mut self, x: i32, y: i32, dx: i32, dy: i32) -> bool {
        if self.ask.is_some() || self.disk.exporting.is_some() {
            return false;
        }
        let changed = self.scrolled(x, y, dx, dy);
        if changed {
            self.rehover(x, y);
        }
        changed
    }

    fn scrolled(&mut self, x: i32, y: i32, dx: i32, dy: i32) -> bool {
        if self.screen == Screen::Settings {
            return self.scroll_settings(x, y, dy);
        }
        if self.roll_shown() && self.roll_rect().contains(x, y) {
            return self.roll.scroll_by(dx, dy);
        }
        if self.browser_shown() && self.browser_rect().contains(x, y) {
            return self.browser.scroll(dy);
        }
        if self.browser_big() {
            return false;
        }
        if self.core_shown() && self.view.aside == Aside::Wave && (self.wave_body().contains(x, y) || self.wave_lane().contains(x, y)) {
            return self.wave_wheel(x, dx, dy);
        }
        if self.core_shown() && (self.face_rect().contains(x, y) || self.over_big(x, y)) {
            let (fx, fy) = self.face_point(x, y);
            let changed = self.face_wheel(fx, fy, dy);
            if changed {
                self.commit_gesture(WHEEL_REST);
                self.remember();
            }
            return changed;
        }
        if self.core_shown() && self.rack_rect().contains(x, y) {
            return self.rack.wheel(dy, self.project.rack(self.host()));
        }
        let mut changed = false;
        if self.track_viewport().contains(x, y) {
            let scroll = (self.timeline.track_scroll - dy).clamp(0, self.max_track_scroll());
            changed |= scroll != self.timeline.track_scroll;
            self.timeline.track_scroll = scroll;
        }
        if self.lanes_region().contains(x, y) || self.ruler_strip().contains(x, y) {
            let scroll = (self.timeline.grid_scroll - dx).max(0);
            changed |= scroll != self.timeline.grid_scroll;
            self.timeline.grid_scroll = scroll;
        }
        changed
    }

    pub(super) fn moved(&mut self, x: i32, y: i32) -> bool {
        if self.drag.is_some() {
            return self.drag_moved(x, y);
        }
        if self.ruler.dragging() {
            let (lane, zone) = (self.ruler_lane(), self.ruler_zone());
            return self.ruler.moved(lane, zone, x, y);
        }
        if self.browser.dragging() {
            let (bx, by) = self.browser_point(x, y);
            let changed = self.browser.moved(bx, by, !self.browser_big());
            if Rect::new(0, 0, self.width, self.height).contains(x, y) {
                return changed;
            }
            let Some((path, ghost)) = self.browser.carry() else {
                return changed;
            };
            self.requests.push(Request::Drag { path, ghost });
            return true;
        }
        if self.roll.dragging() {
            let (rx, ry) = self.roll_point(x, y);
            return self.roll_moved(rx, ry);
        }
        if self.face.dragging() {
            let (fx, fy) = self.face_point(x, y);
            return self.face_moved(fx, fy);
        }
        if self.lfo_face.dragging() {
            return self.lfo_moved(x, y);
        }
        if self.lane_sketch.dragging() {
            return self.sketch_moved(x, y);
        }
        if self.wave_face.dragging() {
            return self.wave_moved(x, y);
        }
        if self.lanes.dragging() {
            let (lx, ly) = self.lanes_point(x, y);
            return self.lanes_moved(lx, ly);
        }
        if self.tv.dragging() {
            let (fx, fy) = self.face_point(x, y);
            return self.tv_moved(fx, fy);
        }
        if self.picture.dragging() {
            return self.picture_moved(x, y);
        }
        if self.rack.dragging() {
            return self.rack_moved(x, y);
        }
        if let Some(deck) = self.active_palette() {
            let (px, py) = self.palette_point(deck, x, y);
            let changed = self.palettes[deck].moved(px, py, &mut self.project);
            if changed && self.live.hold {
                self.live.latched = self.palettes[deck].playing(&self.project).map(|chord| Latch { chord, tracks: self.chord_tracks() });
            }
            let hovered = self.palettes[deck].hover(Some((px, py)));
            let outside = !self.palette_rect(deck).contains(x, y);
            if let Some(chord) = self.palettes[deck].lift(&self.project, outside) {
                self.drag = Some(Drag::Chord(ChordDrag { chord, at: (x, y) }));
                return true;
            }
            return changed | hovered;
        }
        self.rehover(x, y)
    }

    pub(super) fn press(&mut self, x: i32, y: i32, clicks: i32) -> bool {
        let target = self.hover_at(x, y);
        if self.disk.exporting.is_some() {
            if target == Hover::Cancel {
                self.cancel_export();
            }
            self.rehover(x, y);
            return true;
        }
        if self.ask.is_some() {
            if matches!(target, Hover::Confirm | Hover::Cancel) || !self.ask_rect().contains(x, y) {
                self.answer(target == Hover::Confirm);
            }
            self.rehover(x, y);
            return true;
        }
        if let Some(track) = self.rename.as_ref().map(|rename| rename.track) && target == Hover::Tile(track) && tile_title(self.track_tile(self.track_slot(track))).contains(x, y) {
            return false;
        }
        if self.typing_port() && target == Hover::Setting(settings::PORT) && clicks < 2 {
            return false;
        }
        let left = self.face.leave();
        if left && target == Hover::Face {
            let (fx, fy) = self.face_point(x, y);
            if face::readout_rect(self.readout_rows()).contains(fx, fy) {
                self.rehover(x, y);
                return true;
            }
        }
        let finished = left | self.finish_typing() | (target != Hover::Browser && self.browser.unpick());
        let changed = match self.modal_press(target, x, y, clicks) {
            Some(changed) => changed,
            None => self.press_on(target, x, y, clicks),
        };
        self.rehover(x, y);
        finished | changed
    }

    fn modal_press(&mut self, target: Hover, x: i32, y: i32, clicks: i32) -> Option<bool> {
        if self.face.panel_open() {
            let (fx, fy) = self.face_point(x, y);
            return Some(self.face_down(fx, fy, clicks));
        }
        if self.rack.panel_open() {
            return Some(self.rack_down(x, y, clicks));
        }
        if self.lfo_face.panel_open() && let Some(row) = self.lfo_strip {
            return Some(self.lfo_down(row, x, y));
        }
        if self.save.is_some() {
            self.save = match target {
                Hover::SaveOption(index) => Some(SavePanel { pressed: Some(index) }),
                _ => None,
            };
            return Some(true);
        }
        if let Some(panel) = self.stock {
            self.stock = match target {
                Hover::StockOption(index) => Some(StockPanel { pressed: Some(index), ..panel }),
                _ => None,
            };
            return Some(true);
        }
        let menu = self.menu.take()?;
        if let Hover::Item(index) = target {
            match self.delay_bar(menu, index) {
                Some(track) => self.delay_down(track, y, clicks >= 2),
                None => self.choose(menu, index),
            }
        }
        Some(true)
    }

    fn press_on(&mut self, target: Hover, x: i32, y: i32, clicks: i32) -> bool {
        let reset = clicks >= 2;
        match target {
            Hover::Button(button) => {
                self.tap(button);
                true
            }
            Hover::Curve(curve) => self.curve_down(curve, x, y, clicks),
            Hover::Lfo(row) => self.lfo_down(row, x, y),
            Hover::Clear(lane) => self.clear_lane(lane),
            Hover::Lane(_) => self.track_lane_down(x, y),
            Hover::Ruler => self.ruler_down(x, y, clicks),
            Hover::Palette(deck) => {
                let (px, py) = self.palette_point(deck, x, y);
                let changed = self.palettes[deck].down(px, py, &mut self.project);
                if self.live.hold && let Some(chord) = self.palettes[deck].playing(&self.project) {
                    let latch = Latch { chord, tracks: self.chord_tracks() };
                    self.live.latched = if self.live.latched.as_ref() == Some(&latch) { None } else { Some(latch) };
                }
                changed
            }
            Hover::Feedback => {
                self.requests.push(Request::OpenLink { url: String::from(FEEDBACK_URL) });
                false
            }
            Hover::Tempo => self.tempo_down(y, reset),
            Hover::Zoom => self.zoom_down(x, y, reset),
            Hover::Face => {
                let (fx, fy) = self.face_point(x, y);
                self.face_down(fx, fy, clicks)
            }
            Hover::Load => {
                self.requests.push(Request::Open { path: None });
                false
            }
            Hover::Rack => self.rack_down(x, y, clicks),
            Hover::Roll => {
                let (rx, ry) = self.roll_point(x, y);
                self.roll_down(rx, ry, clicks)
            }
            Hover::Lanes => self.lanes_down(x, y),
            Hover::Browser => self.browser_down(x, y, clicks),
            Hover::Setting(name) => self.settings_press(name, y, reset),
            Hover::Version(index) => self.restore_version(index),
            Hover::Edit(index) => self.restore_edit(index),
            Hover::Track(index, button) => self.press_track(index, button),
            Hover::Append(slot) => self.press_append(slot),
            Hover::Fold(index) => self.toggle_fold(index),
            Hover::Tile(index) => {
                self.tile_down(index, x, y);
                false
            }
            Hover::Volume(index) => self.volume_down(index, y, reset),
            Hover::Master => self.master_down(y, reset),
            Hover::TrackSend(track, slot) => self.track_send_down(track, slot, y, reset),
            Hover::Clip(track, index, hit) => self.clip_down(track, index, hit, x, y),
            Hover::Column => {
                if self.shift {
                    self.sweep_tracks(x, y, None);
                }
                false
            }
            Hover::Wave => self.wave_down(x, y),
            Hover::WaveHead => self.head_down(x),
            Hover::WaveBody(_) => self.body_down(x, y, reset),
            Hover::WaveFade(slope, part) => self.fade_down(slope, part, x, reset),
            Hover::WaveLane(hit) => self.lane_down(hit, x, y),
            Hover::WaveZoom => self.wave_zoom_down(x, y, reset),
            Hover::WaveLoop => self.toggle_wave_loop(reset),
            Hover::WaveReverse => self.reverse_clip(),
            Hover::WaveSave => self.save_clip_wave(),
            Hover::WaveAll => self.match_clips(),
            Hover::SaveOption(_) | Hover::StockOption(_) | Hover::Hertz(_) | Hover::Item(_) | Hover::Confirm | Hover::Cancel | Hover::Clock | Hover::Nothing => false,
        }
    }

    pub(super) fn tap(&mut self, button: Button) {
        match button {
            Button::Menu(menu) => self.menu = Some(menu),
            Button::Chord => self.view.palette_shown = !self.view.palette_shown,
            Button::Automation => self.show_auto(self.view.auto.next()),
            Button::Lock => self.project.lock = !self.project.lock,
            Button::Higher => {
                self.project.transposed(1);
            }
            Button::Lower => {
                self.project.transposed(-1);
            }
            Button::Big => self.view.big = !self.view.big,
            Button::Group => {
                self.group_or_ungroup();
            }
            Button::Bounce => {
                self.start_bounce();
            }
            Button::Flatten => {
                self.start_flatten();
            }
            Button::Play => self.play(),
            Button::Stop => {
                if self.playing {
                    self.stop();
                } else if self.pool.audition.is_some() {
                    self.stop_audition();
                } else {
                    self.rewind();
                }
            }
            Button::Record => self.record(),
            Button::Hold => {
                self.live.hold = !self.live.hold;
                if !self.live.hold {
                    self.live.latched = None;
                    self.release_keys(|note| !note.down);
                }
            }
            Button::Metronome => self.project.metronome ^= true,
            Button::Keyboard => {
                self.live.keyboard = !self.live.keyboard;
                if !self.live.keyboard {
                    self.release_keys(|_| true);
                }
            }
            Button::Save => self.save = Some(SavePanel { pressed: None }),
            Button::Stock => self.open_stock(),
            Button::Close => self.screen = Screen::Main,
            Button::CopyMcp => self.copy_config(),
            Button::Show(aside) => self.set_aside(if self.view.aside == aside { Aside::Blank } else { aside }),
            Button::MasterName => self.toggle_master(),
            Button::Send(slot) => self.tap_send(self.send_ordinal(slot)),
            Button::Cycle => self.view.bank = (self.view.bank + 1) % SEND_BANKS,
        }
    }

    fn busy(&self) -> bool {
        self.screen != Screen::Main || self.dragging() || self.ask.is_some() || self.disk.exporting.is_some() || self.menu.is_some() || self.save.is_some() || self.stock.is_some() || self.face.panel_open() || self.rack.panel_open() || self.lfo_face.panel_open()
    }

    pub(super) fn middle_press(&mut self, x: i32, y: i32) -> bool {
        if self.busy() {
            return false;
        }
        self.view.big = !self.view.big;
        self.rehover(x, y);
        self.remember();
        true
    }

    pub(super) fn right_press(&mut self, x: i32, y: i32) -> bool {
        if self.busy() {
            return false;
        }
        let finished = self.finish_typing();
        let changed = match self.hover_at(x, y) {
            Hover::Face => {
                let (fx, fy) = self.face_point(x, y);
                self.picture_right(fx, fy) || self.arm_at(fx, fy)
            }

            Hover::Rack => self.rack_right(x, y),
            Hover::Button(Button::Automation) if self.view.auto != Auto::Off => {
                self.show_auto(Auto::Off);
                true
            }
            Hover::Volume(track) => self.arm_volume(track),
            Hover::TrackSend(track, slot) => self.arm_send(track, slot),
            Hover::Master => self.arm_return(),
            Hover::Lfo(lane) => self.arm_lfo(lane, x, y),
            Hover::Lane(_) | Hover::Clip(..) | Hover::Curve(_) => self.zoom_at(x, y),
            Hover::Roll => self.roll_zoom_at(x, y),
            Hover::WaveBody(_) | Hover::WaveFade(..) => self.wave_zoom_at(x, y),
            _ => false,
        };
        self.rehover(x, y);
        finished | changed
    }

    pub(super) fn choose(&mut self, menu: Menu, index: usize) {
        match menu {
            Menu::File => self.perform(FILE_ITEMS[index].0),
            Menu::Export => self.export(EXPORT_ITEMS[index].0),
            Menu::Recent => {
                if let Some(path) = self.disk.recent.get(index) {
                    self.requests.push(Request::Open { path: Some(path.clone()) });
                }
            }
            Menu::Snap => self.timeline.snap = grid::SNAPS[index],
            Menu::Key => self.project.root = Root::ALL[index],
            Menu::Scale => self.project.scale = ScaleName::ALL[index],
            Menu::Fit => {
                if let Some(fit) = self.fits.get(index) {
                    (self.project.root, self.project.scale) = (fit.root, fit.scale);
                }
            }
            Menu::Add(track) => match ADD_ITEMS.get(index) {
                Some((fresh, _)) => {
                    if self.insert_track(track + 1, fresh(), true) {
                        self.select(track + 1);
                    }
                }
                None => self.menu = Some(Menu::Follow(track)),
            },
            Menu::Follow(track) => {
                if let Some(&source) = self.project.follow_sources(track).get(index) {
                    let follows = &mut self.project.tracks[track].follows;
                    *follows = (*follows != Some(source)).then_some(source);
                }
            }
        }
    }

    pub(super) fn hover_at(&self, x: i32, y: i32) -> Hover {
        if self.disk.exporting.is_some() {
            return if tile_inner(self.export_button()).contains(x, y) { Hover::Cancel } else { Hover::Nothing };
        }
        if self.ask.is_some() {
            return ASK_BUTTONS.iter().enumerate().find(|(index, _)| tile_inner(self.ask_button(*index)).contains(x, y)).map_or(Hover::Nothing, |(_, hover)| *hover);
        }
        if file_inner().contains(x, y) {
            return Hover::Button(Button::Menu(Menu::File));
        }
        if let Some(menu) = self.menu {
            return match (0..self.menu_labels(menu).len()).find(|&index| self.menu_item(menu, index).contains(x, y)) {
                Some(index) => Hover::Item(index),
                None => Hover::Nothing,
            };
        }
        if self.save.is_some() {
            let labels = self.save_labels();
            return match face::option_at(self.save_panel(&labels), &labels, x, y) {
                Some(index) => Hover::SaveOption(index),
                None => Hover::Nothing,
            };
        }
        if self.stock.is_some() {
            let labels = self.stock_labels();
            return match face::option_at(self.stock_panel(&labels), &labels, x, y) {
                Some(index) => Hover::StockOption(index),
                None => Hover::Nothing,
            };
        }
        match self.screen {
            Screen::Settings => {
                if close_inner(self.width).contains(x, y) {
                    return Hover::Button(Button::Close);
                }
                let hover = self.settings_hover(x, y);
                if hover != Hover::Nothing {
                    return hover;
                }
            }
            Screen::Main => {
                if let Some(index) = (0..HEADER.len()).find(|&index| (HEADER[index].shown)(self) && tile_inner(self.header_cell(index)).contains(x, y)) {
                    return HEADER[index].hover;
                }
                if self.feedback_shown() && tile_inner(self.feedback_cell()).contains(x, y) {
                    return Hover::Feedback;
                }
                if self.browser_big() {
                    return if self.browser_rect().contains(x, y) { Hover::Browser } else { Hover::Nothing };
                }
                if self.view.palette_shown && !self.view.big && let Some(deck) = self.palette_at(x, y) {
                    return Hover::Palette(deck);
                }
                if self.roll_shown() && self.roll_rect().contains(x, y) {
                    return Hover::Roll;
                }
                if self.ruler_readout().contains(x, y) {
                    return Hover::Zoom;
                }
                if self.ruler_strip().contains(x, y) {
                    return Hover::Ruler;
                }
                if self.core_shown() && let Some(hover) = self.stage_hover_at(x, y) {
                    return hover;
                }
                if let Some(hover) = self.strip_hover_at(x, y) {
                    return hover;
                }
                if self.column_bounds().contains(x, y) {
                    return Hover::Column;
                }
            }
        }
        Hover::Nothing
    }

    pub(super) fn set_hover(&mut self, hover: Hover) -> bool {
        let changed = hover != self.hover;
        self.hover = hover;
        changed
    }
}

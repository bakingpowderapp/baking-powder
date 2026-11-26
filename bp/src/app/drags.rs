use super::*;
use super::keys::Rename;
use super::presets::{SavePanel, StockPanel};
use super::transport::TEMPO_FINE_STEPS_PER_BPM;
use crate::project::{LEAST_BPM, MOST_BPM};

const TEMPO_PX_PER_STEP: i32 = 2;
const SEND_THROW: f32 = 100.0;
const DELAY_PX_PER_STEP: i32 = 2;
const DELAY_FINE_STEPS_PER_MS: f32 = 10.0;

#[derive(Clone, Copy)]
pub(super) struct ZoomDrag {
    pub(super) origin: (i32, i32),
    pub(super) at_x: i32,
    pub(super) beat_w: f64,
    pub(super) centre: f64,
}

#[derive(Clone, Copy)]
pub(super) struct ZoomAt {
    pub(super) origin: (i32, i32),
    pub(super) beat_w: f64,
    pub(super) beat: f64,
}

impl App {
    pub(super) fn drag_moved(&mut self, x: i32, y: i32) -> bool {
        match &mut self.drag {
            Some(Drag::Chord(drag)) => {
                drag.at = (x, y);
                true
            }
            Some(Drag::Auto { .. }) => self.auto_moved(x, y),
            Some(Drag::Ghost(drag)) => {
                let drag = *drag;
                self.ghost_moved(drag, x, y)
            }
            Some(Drag::Contrast(_) | Drag::ScreenZoom(_) | Drag::Preview(_) | Drag::Buffer(_)) => self.settings_drag(y),
            Some(Drag::Tempo(drag)) => {
                drag.y = y;
                let steps = f64::from((drag.origin_y - y) / TEMPO_PX_PER_STEP);
                let bpm = if self.shift { ((drag.origin * TEMPO_FINE_STEPS_PER_BPM).round() + steps) / TEMPO_FINE_STEPS_PER_BPM } else { (drag.origin + steps).round() };
                let bpm = bpm.clamp(LEAST_BPM, MOST_BPM);
                let changed = bpm != self.project.bpm;
                self.project.bpm = bpm;
                changed
            }
            Some(Drag::Zoom(drag)) => {
                drag.at_x = x;
                let beat_w = (drag.beat_w * (f64::from(y - drag.origin.1) / grid::ZOOM_PX_PER_DOUBLE).exp2()).clamp(grid::BEAT_W_MIN, grid::BEAT_W_MAX);
                let centre = drag.centre;
                self.set_view(beat_w, centre)
            }
            Some(Drag::ZoomAt(drag)) => {
                let beat_w = (drag.beat_w * (f64::from(y - drag.origin.1) / grid::ZOOM_PX_PER_DOUBLE).exp2()).clamp(grid::BEAT_W_MIN, grid::BEAT_W_MAX);
                let (beat, x) = (drag.beat, drag.origin.0);
                self.set_view_under(beat_w, beat, x)
            }
            Some(Drag::Track(drag)) => {
                drag.y = y;
                drag.armed |= self.view.auto == Auto::Off && (y - drag.origin_y).abs() >= DRAG_ARM_PX;
                drag.armed
            }
            Some(Drag::Clip(drag)) => {
                drag.at = (x, y);
                drag.armed |= (x - drag.origin.0).abs() >= DRAG_ARM_PX || (y - drag.origin.1).abs() >= DRAG_ARM_PX;
                let key = (drag.track, self.project.tracks[drag.track].clips[drag.index].start);
                if drag.armed && !self.selection.clips.contains(&key) {
                    self.selection.clips.push(key);
                }
                drag.armed
            }
            Some(Drag::Region(drag)) => {
                let drag = *drag;
                self.region_moved(drag, x, y)
            }
            Some(Drag::Marquee(marquee)) => {
                marquee.at = (x, y);
                marquee.armed |= (x - marquee.origin.0).abs() >= DRAG_ARM_PX || (y - marquee.origin.1).abs() >= DRAG_ARM_PX;
                if !marquee.armed {
                    return false;
                }
                let marquee = marquee.clone();
                self.swept_into(&marquee);
                true
            }
            Some(Drag::TrackMarquee(marquee)) => {
                marquee.at = (x, y);
                marquee.armed |= (x - marquee.origin.0).abs() >= DRAG_ARM_PX || (y - marquee.origin.1).abs() >= DRAG_ARM_PX;
                if !marquee.armed {
                    return false;
                }
                let (sweep, mut picked) = (grid::marquee(marquee.origin, marquee.at), marquee.before.clone());
                for track in self.swept_tracks(sweep) {
                    if track != self.selection.track && !picked.contains(&track) {
                        picked.push(track);
                    }
                }
                self.selection.track_picks = picked;
                true
            }
            Some(Drag::Volume(gang)) => {
                let inner = volume_inner(Rect::new(0, 0, TILE, TILE));
                let mut changed = false;
                for &(track, origin) in &gang.origins {
                    let gain = fader_gain(origin, gang.origin_y, y, inner);
                    changed |= gain != self.project.tracks[track].gain;
                    self.project.tracks[track].gain = gain;
                }
                changed
            }
            Some(Drag::Delay(gang)) => {
                let steps = ((gang.origin_y - y) / DELAY_PX_PER_STEP) as f32;
                let per_ms = if self.shift { DELAY_FINE_STEPS_PER_MS } else { 1.0 };
                let mut changed = false;
                for &(track, origin) in &gang.origins {
                    let delay = (((origin * per_ms).round() + steps) / per_ms).clamp(-DELAY_MS, DELAY_MS);
                    changed |= delay != self.project.tracks[track].delay_ms;
                    self.project.tracks[track].delay_ms = delay;
                }
                changed
            }
            Some(Drag::Master(drag)) => {
                let gain = fader_gain(drag.origin, drag.origin_y, y, tile_inner(self.master_volume()));
                self.set_master_gain(gain)
            }
            Some(Drag::Send(drag)) => {
                let dy = drag.gang.origin_y - y;
                if !drag.moved && dy.abs() < DRAG_ARM_PX {
                    return false;
                }
                drag.moved = true;
                let mut changed = false;
                for &(track, origin) in &drag.gang.origins {
                    let amount = (origin + dy as f32 / SEND_THROW).clamp(0.0, 1.0);
                    let send = &mut self.project.tracks[track].sends[drag.send];
                    changed |= amount != *send;
                    *send = amount;
                }
                changed
            }
            Some(Drag::Head) => self.head_to(x),
            Some(Drag::WaveZoom(drag)) => {
                drag.at_x = x;
                let (origin_y, per_column, centre) = (drag.origin.1, drag.per_column, drag.centre);
                self.zoom_wave(per_column / (f64::from(y - origin_y) / grid::ZOOM_PX_PER_DOUBLE).exp2(), centre)
            }
            Some(Drag::WaveZoomAt(drag)) => {
                let (origin, per_column, frame) = (drag.origin, drag.per_column, drag.frame);
                self.zoom_wave_under(per_column / (f64::from(y - origin.1) / grid::ZOOM_PX_PER_DOUBLE).exp2(), frame, origin.0)
            }
            Some(Drag::Select(drag)) => {
                drag.at_x = x;
                drag.armed |= (x - drag.origin_x).abs() >= DRAG_ARM_PX;
                if !drag.armed {
                    return false;
                }
                let origin_x = drag.origin_x;
                self.select_wave(origin_x, x)
            }
            Some(Drag::Span(drag)) => {
                let drag = *drag;
                self.span_moved(drag, x, y)
            }
            Some(Drag::Fade(drag)) => {
                let drag = *drag;
                self.fade_moved(drag, x, y)
            }
            None => false,
        }
    }

    pub(super) fn release(&mut self, x: i32, y: i32) -> bool {
        let mut changed = self.drag_released();
        if let Some(SavePanel { pressed: Some(pressed) }) = self.save {
            if self.hover_at(x, y) == Hover::SaveOption(pressed) {
                self.save = None;
                self.save_preset(pressed);
            } else {
                self.save = Some(SavePanel { pressed: None });
            }
            changed = true;
        }
        if let Some(panel @ StockPanel { pressed: Some(pressed), .. }) = self.stock {
            if self.hover_at(x, y) == Hover::StockOption(pressed) {
                self.stock_pick(pressed);
            } else {
                self.stock = Some(StockPanel { pressed: None, ..panel });
            }
            changed = true;
        }
        let (fx, fy) = self.face_point(x, y);
        let (face_changed, wave_changed) = (self.face_up(fx, fy), self.wave_up(x, y));
        changed |= face_changed | wave_changed | self.lfo_up(x, y) | self.picture_up() | self.tv_up() | self.sketch_up() | self.lanes.up();
        let (rx, ry) = self.rack_point(x, y);
        let (host, offer) = (self.host(), self.offer());
        let (rack_changed, action) = self.rack.up(rx, ry, self.project.rack(host), offer);
        changed |= rack_changed;
        if rack_changed && !self.rack.picked().is_empty() {
            self.face.unpick();
            self.unpick_span();
        } else if wave_changed && self.wave_face.picking() {
            self.face.unpick();
            self.rack.unpick();
        }
        if let Some(action) = action {
            self.rack_action(action);
            changed = true;
        }
        let (bx, by) = self.browser_point(x, y);
        let (browser_changed, action) = self.browser.up(bx, by, self.site_at(x, y).is_some());
        changed |= browser_changed;
        if let Some(action) = action {
            self.browser_act_at(action, x, y);
            changed = true;
        }
        changed |= self.roll_up(x, y);
        match self.ruler.up() {
            Some(Release::Click(beat)) => {
                self.place_playhead(beat);
                changed = true;
            }
            Some(Release::Edit) => {
                self.mark_loop();
                changed = true;
            }
            None => {}
        }
        if let Some(deck) = self.active_palette() {
            let (px, py) = self.palette_point(deck, x, y);
            let up = self.palettes[deck].up(px, py, &mut self.project);
            match up {
                Up::Big => self.view.palette_big = !self.view.palette_big,
                Up::Save => self.save_chords(deck),
                Up::Nothing | Up::Changed => {}
            }
            changed |= up != Up::Nothing;
        }
        self.rehover(x, y);
        changed
    }

    fn drag_released(&mut self) -> bool {
        let Some(drag) = self.drag.take() else {
            return false;
        };
        match drag {
            Drag::Contrast(_) | Drag::ScreenZoom(_) | Drag::Preview(_) | Drag::Tempo(_) | Drag::Zoom(_) | Drag::ZoomAt(_) | Drag::Volume(_) | Drag::Delay(_) | Drag::Master(_) | Drag::Auto { .. } | Drag::Head | Drag::WaveZoom(_) | Drag::WaveZoomAt(_) | Drag::Fade(_) => {}
            Drag::Region(drag) => {
                if !drag.armed {
                    self.mark_start(self.ruler_lane().floored(drag.origin.0).max(0));
                }
            }
            Drag::Marquee(marquee) => {
                if !marquee.armed && let Some((track, index)) = marquee.hit {
                    self.toggle_clip(track, index);
                }
            }
            Drag::Select(drag) => {
                if !drag.armed {
                    self.clear_wave_selection();
                }
            }
            Drag::Span(drag) => self.span_released(drag),
            Drag::Ghost(drag) => self.ghost_released(drag),
            Drag::Buffer(drag) => self.buffer_released(drag),
            Drag::Send(drag) => {
                if !drag.moved {
                    self.select_host(Host::Send(drag.send));
                }
            }
            Drag::Track(drag) => {
                if drag.armed {
                    self.drop_track(drag);
                } else if drag.title && drag.track == self.selection.track && self.monitored.is_none() {
                    self.rename = Some(Rename { track: drag.track, text: String::new() });
                } else {
                    self.select(drag.track);
                }
            }
            Drag::Clip(drag) => {
                if drag.armed {
                    self.drop_clip(drag);
                } else {
                    self.pick_clip(drag.track, drag.index);
                    self.mark_start(self.project.tracks[drag.track].clips[drag.index].start);
                }
            }
            Drag::TrackMarquee(marquee) => {
                if !marquee.armed && let Some(track) = marquee.hit {
                    self.pick_track(track);
                }
            }
            Drag::Chord(drag) => {
                if let Some((track, lane)) = self.lane_target(drag.at)
                    && !self.project.tracks[track].audio
                {
                    let clip = self.chord_clip(drag, lane.bar_at(drag.at.0).max(0));
                    if clip.start <= LONGEST_TICKS - clip.length {
                        let bpm = self.project.bpm;
                        self.project.tracks[track].place(clip, bpm);
                    }
                }
            }
        }
        true
    }

    pub(super) fn zoom_down(&mut self, x: i32, y: i32, reset: bool) -> bool {
        let centre = self.centre_beat();
        if reset {
            self.set_view(grid::BEAT_W, centre);
        } else {
            self.drag = Some(Drag::Zoom(ZoomDrag { origin: (x, y), at_x: x, beat_w: self.timeline.beat_w, centre }));
        }
        true
    }

    pub(super) fn zoom_at(&mut self, x: i32, y: i32) -> bool {
        self.drag = Some(Drag::ZoomAt(ZoomAt { origin: (x, y), beat_w: self.timeline.beat_w, beat: self.timeline.beat_under(x) }));
        true
    }
}

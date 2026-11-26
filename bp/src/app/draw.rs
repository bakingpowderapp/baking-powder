use super::*;
use super::clips::CUT_HOLD_SECONDS;
use super::presets::{SavePanel, StockPanel};
use super::region::Region;


impl App {
    pub fn draw(&mut self, width: i32, height: i32) -> &[Fill] {
        self.face.name_list(self.track_names(), self.withheld_name());
        self.face.asleep(Act::Flatten, self.flat_refusal());
        self.width = width;
        self.height = height;
        self.list.clear();
        self.list.fill(Rect::new(0, 0, width, height), Tone::Ground);
        if self.small() {
            self.draw_sorry();
            return self.list.fills();
        }
        match self.screen {
            Screen::Main => self.draw_main(),
            Screen::Settings => self.draw_settings(),
        }
        self.draw_header();
        if let Some(menu) = self.menu {
            if menu.root() != menu {
                self.draw_menu(menu.root());
            }
            self.draw_menu(menu);
        }
        self.draw_ask();
        self.draw_export();
        self.list.fills()
    }

    pub(super) fn draw_ask(&mut self) {
        let Some((name, (title, description, label))) = self.ask.as_ref().map(|ask| (String::from(ask.name()), ask.words())) else {
            return;
        };
        let (rect, message) = (self.ask_rect(), self.ask_message());
        self.list.fill(rect, Tone::Ground);
        self.list.frame(message, BORDER, Tone::Ink100);
        let inner = tile_inner(message);
        let (x, mut y) = (inner.x + TEXT_PAD, inner.y + TEXT_PAD);
        self.list.text(x, y, title, Tone::Ink50, TEXT_SCALE);
        y += font::GLYPH_H * TEXT_SCALE + ASK_LEADING;
        self.list.text(x, y, &font::shortened(&name, font::glyphs_in(inner.w - 2 * TEXT_PAD, ASK_NAME_SCALE)), Tone::Ink100, ASK_NAME_SCALE);
        y += font::GLYPH_H * ASK_NAME_SCALE + ASK_LEADING;
        self.list.text(x, y, description, Tone::Ink75, face::LABEL_SCALE);
        for (index, (hover, label)) in ASK_BUTTONS.iter().zip([CANCEL, label]).enumerate() {
            self.draw_control(self.ask_button(index), BORDER, label, false, self.hover == *hover, false);
        }
    }

    pub(super) fn draw_export(&mut self) {
        let Some((title, name, fraction, age)) = self.disk.exporting.as_ref().map(|exporting| (exporting.title.clone(), exporting.name.clone(), exporting.rendering.progress(), exporting.age)) else {
            return;
        };
        let (rect, message) = (self.export_rect(), self.export_message());
        self.list.fill(rect, Tone::Ground);
        self.list.frame(message, BORDER, Tone::Ink100);
        let inner = tile_inner(message);
        let (x, mut y) = (inner.x + TEXT_PAD, inner.y + TEXT_PAD);
        self.list.text(x, y, &font::shortened(&title, font::glyphs_in(inner.w - 2 * TEXT_PAD, TEXT_SCALE)), Tone::Ink50, TEXT_SCALE);
        y += font::GLYPH_H * TEXT_SCALE + ASK_LEADING;
        self.list.text(x, y, &font::shortened(&name, font::glyphs_in(inner.w - 2 * TEXT_PAD, ASK_NAME_SCALE)), Tone::Ink100, ASK_NAME_SCALE);
        if fraction > 0.0 {
            let percent = format!("{}%", (fraction * 100.0).floor() as i32);
            self.draw_meter(self.export_bar(), CONTROL_BORDER, &percent, &percent, fraction, false, false);
        } else {
            self.draw_warming(self.export_bar(), CONTROL_BORDER, age);
        }
        self.draw_control(self.export_button(), BORDER, CANCEL, false, self.hover == Hover::Cancel, false);
    }

    pub fn screenshot(&mut self, width: i32, height: i32, scale: i32) -> Vec<u8> {
        let window = (self.width, self.height);
        let theme = self.theme();
        let mut png = Png::new(width, height, scale);
        png.paint(self.draw(width, height), theme);
        (self.width, self.height) = window;
        png.finish()
    }

    pub(super) fn draw_main(&mut self) {
        if self.roll_big() {
            self.list.frame(self.stage_rect(), BORDER, Tone::Ink100);
            self.draw_roll();
            self.draw_ruler();
            return;
        }
        if self.browser_big() {
            self.list.frame(self.browser_cell(), BORDER, Tone::Ink100);
            self.draw_browser();
            return;
        }
        let seam_bottom = self.seam_bottom();
        let bottom = self.arrangement_bottom();
        self.list.fill(Rect::new(0, RAIL_H, BORDER, (seam_bottom - RAIL_H).max(0)), Tone::Ink100);
        if !self.view.big {
            self.list.frame(self.stage_rect(), BORDER, Tone::Ink100);
        }
        self.draw_tracks();
        self.draw_marquee();
        self.draw_loop_edges();
        self.draw_cut_fuse();
        self.list.fill(Rect::new(self.width - BORDER, RAIL_H, BORDER, (seam_bottom - RAIL_H).max(0)), Tone::Ink100);
        self.draw_ruler();
        let lane = self.lane(self.track_tile(0));
        let x = lane.x_of_beat(self.playhead);
        if self.playing && x >= lane.rect.x && x < self.width - BORDER {
            self.list.fill(Rect::new(x, RAIL_H + BORDER, grid::PLAYHEAD_W, (bottom - RAIL_H - BORDER).max(0)), Tone::Ink100);
        }
        if self.view.big {
            self.draw_clip_ghost();
            return;
        }
        if self.view.palette_shown {
            self.draw_palette();
        } else if self.roll_shown() {
            self.draw_roll();
        } else {
            self.draw_core();
            self.draw_aside();
            self.draw_master();
            self.draw_save_panel();
            self.draw_stock_panel();
        }
        self.draw_clip_ghost();
        self.draw_chord_ghost();
        self.draw_browser_ghost();
    }

    pub(super) fn draw_marquee(&mut self) {
        if let Some(Drag::Marquee(marquee)) = &self.drag
            && marquee.armed
        {
            let sweep = grid::marquee(marquee.origin, marquee.at);
            self.list.clip(Some(self.lanes_bounds()));
            grid::draw_marquee(&mut self.list, sweep);
        }
        if let Some(Drag::TrackMarquee(marquee)) = &self.drag
            && marquee.armed
        {
            let sweep = grid::marquee(marquee.origin, marquee.at);
            self.list.clip(Some(self.column_bounds()));
            grid::draw_marquee(&mut self.list, sweep);
        }
        self.list.clip(None);
    }

    pub(super) fn draw_loop_edges(&mut self) {
        let (lane, bounds) = (self.ruler_lane(), self.lanes_bounds());
        self.ruler.draw_edges(&mut self.list, lane, bounds);
    }

    pub(super) fn draw_cut_fuse(&mut self) {
        let (lane, band) = (self.ruler_lane(), self.lanes_bounds());
        if let Some(drag) = self.clip_drag().filter(|drag| !drag.armed && drag.held > 0.0) {
            let tick = self.cut_tick(drag);
            let h = (band.h as f32 * (drag.held / CUT_HOLD_SECONDS).min(1.0)).round() as i32;
            ruler::draw_cue(&mut self.list, lane, band, tick, h, Tone::Ink25);
        }
        if let Some((tick, _)) = self.cut_flash {
            ruler::draw_cue(&mut self.list, lane, band, tick, band.h, Tone::Ink100);
        }
    }

    pub(super) fn draw_chord_ghost(&mut self) {
        let Some(Drag::Chord(drag)) = self.drag else {
            return;
        };
        match self.lane_target(drag.at) {
            Some((track, lane)) => {
                let clip = self.chord_clip(drag, lane.bar_at(drag.at.0).max(0));
                let range = self.project.tracks[track].pitch_range();
                let bpm = self.project.bpm;
                self.list.clip(Some(lane.rect.intersect(self.track_viewport())));
                lane.draw_clip(&mut self.list, &clip, clip.start, clip.length, ClipLook::Plain, false, range, None, bpm);
                self.list.clip(None);
            }
            None => {
                let w = self.timeline.clip_w(TICKS_PER_BAR);
                grid::draw_ghost(&mut self.list, drag.at, w / 2, w, TILE - 2 * BORDER);
            }
        }
    }

    pub(super) fn draw_clip_ghost(&mut self) {
        let Some(drag) = self.clip_drag().filter(|drag| drag.armed && !drag.resizing()) else {
            return;
        };
        let phrase: Vec<(usize, Clip)> = self.picked_clips().into_iter().filter(|(track, _)| !self.hidden(*track)).collect();
        match self.clip_shift(drag) {
            Some((by, rows)) => {
                let viewport = self.track_viewport();
                for (track, clip) in phrase {
                    let target = (track as i32 + rows) as usize;
                    let lane = self.lane(self.track_tile(self.track_slot(self.lane_of(target))));
                    let range = self.project.tracks.get(target).and_then(Track::pitch_range);
                    let (wave, bpm) = (self.pool.wave_of(&clip), self.project.bpm);
                    self.list.clip(Some(lane.rect.intersect(viewport)));
                    lane.draw_clip(&mut self.list, &clip, clip.start + by, clip.length, ClipLook::Plain, false, range, wave.as_deref(), bpm);
                }
                self.list.clip(None);
            }
            None => {
                let anchor = self.project.tracks[drag.track].clips[drag.index].start;
                for (track, clip) in phrase {
                    let at = (drag.at.0 + (beats(clip.start - anchor) * self.timeline.beat_w).round() as i32, drag.at.1 + (track as i32 - drag.track as i32) * (TILE - BORDER));
                    let w = self.timeline.clip_w(clip.length);
                    grid::draw_ghost(&mut self.list, at, drag.grab, w, TILE - 2 * BORDER);
                }
            }
        }
    }

    pub(super) fn draw_ruler(&mut self) {
        self.list.fill(self.ruler_cell(), Tone::Ink100);
        self.draw_zoom_cell();
        let lane = self.ruler_lane();
        let start = self.play_from;
        let playhead = self.playing.then_some(self.playhead);
        self.ruler.draw(&mut self.list, lane, start, self.project.end(), playhead, self.settings.bar_numbers);
    }

    pub(super) fn draw_zoom_cell(&mut self) {
        let cell = self.ruler_readout();
        let filled = cell.x + (cell.w as f32 * self.timeline.zoom_travel()).round() as i32;
        self.list.fill(cell, if self.hover == Hover::Zoom { Tone::Ink25 } else { Tone::Ground });
        self.list.fill(Rect::new(cell.x, cell.y, filled - cell.x, cell.h), Tone::Ink100);
        let region = self.selection.region.as_ref().map(Region::label);
        let label = match (&self.drag, self.bar_under) {
            (Some(Drag::Zoom(_) | Drag::ZoomAt(_)), _) => self.bars_label(),
            (Some(Drag::Region(_)), _) => region.unwrap_or_default(),
            (None, Some(bar)) if self.settings.bar_numbers => format!("BAR {bar}"),
            _ => region.unwrap_or_else(|| String::from("ZOOM")),
        };
        let x = cell.x + (cell.w - font::width(&label, 1)) / 2;
        self.list.text_split(x, cell.y + (cell.h - font::GLYPH_H) / 2, &label, filled, Tone::Ground, Tone::Ink100, 1);
    }

    pub(super) fn draw_save_panel(&mut self) {
        let Some(SavePanel { pressed }) = self.save else {
            return;
        };
        let labels = self.save_labels();
        let panel = self.save_panel(&labels);
        let hovered = match self.hover {
            Hover::SaveOption(index) => Some(index),
            _ => None,
        };
        face::draw_options(&mut self.list, panel, &labels, None, hovered, pressed, None);
    }

    pub(super) fn draw_stock_panel(&mut self) {
        let Some(StockPanel { pressed, chosen, .. }) = self.stock else {
            return;
        };
        let labels = self.stock_labels();
        let panel = self.stock_panel(&labels);
        let hovered = match self.hover {
            Hover::StockOption(index) => Some(index),
            _ => None,
        };
        face::draw_options(&mut self.list, panel, &labels, chosen, hovered, pressed, None);
    }

    pub(super) fn draw_aside(&mut self) {
        match self.view.aside {
            Aside::Spectrum => self.draw_spectrum(),
            Aside::Browser => self.draw_browser(),
            Aside::Wave => self.draw_wave(),
            Aside::Pattern => self.draw_lanes(),
            Aside::Picture => self.draw_big(),
            Aside::Blank => {}
        }
    }

    pub(super) fn draw_browser_ghost(&mut self) {
        if !self.browser_shown() {
            return;
        }
        let rect = self.browser_rect();
        self.list.transform((rect.x, rect.y), 1.0);
        self.browser.draw_ghost(&mut self.list);
        self.list.transform((0, 0), 1.0);
    }

    pub(super) fn draw_header(&mut self) {
        self.list.frame(Rect::new(0, 0, self.width, RAIL_H + BORDER), BORDER, Tone::Ink100);
        self.draw_control(file_cell(), BORDER, "File", true, self.hover == Hover::Button(Button::Menu(Menu::File)), self.menu_open(Menu::File));
        match self.screen {
            Screen::Main => {
                self.list.frame(block_cell(), BORDER, Tone::Ink100);
                for (index, head) in HEADER.iter().enumerate() {
                    if !(head.shown)(self) {
                        continue;
                    }
                    let (cell, hover, on) = (self.header_cell(index), self.hover == head.hover, (head.on)(self));
                    match (head.wears)(self) {
                        Wears::Mark(mark) => self.draw_mark_cell(cell, mark, hover, on),
                        Wears::Fold(_, mark) if !self.worded() => self.draw_mark_cell(cell, mark, hover, on),
                        Wears::Word(label) | Wears::Fold(label, _) => self.draw_control(cell, BORDER, &label, false, hover, on),
                        Wears::Readout(text) => self.draw_control(cell, BORDER, &text, false, false, false),
                        Wears::Meter(name, readout, fraction, pressed) => self.draw_meter(cell, BORDER, &name, &readout, fraction, hover, pressed),
                    }
                }
                let shown: Vec<&Head> = HEADER.iter().filter(|head| (head.shown)(self)).collect();
                for pair in shown.windows(2) {
                    if (pair[0].on)(self) && (pair[1].on)(self) {
                        self.list.divide(self.cell_of(pair[0].hover), self.cell_of(pair[1].hover), BORDER);
                    }
                }
                let last = self.last_header_cell();
                let rendering = self.audio.as_ref().and_then(Audio::rendering).filter(|_| last.x + last.w <= self.rev_cell().x + BORDER);
                let copied = self.settings.copied > 0.0;
                if self.disk.saved_note.is_some() || copied {
                    let x = last.x + last.w + HEADER_PAD + TEXT_PAD;
                    let edge = if rendering.is_some() { self.rev_cell() } else { self.feedback_cell() };
                    let room = edge.x - TEXT_PAD - x;
                    let text = if copied { String::from(settings::COPIED) } else { self.disk.saved_text(room) };
                    if font::width(&text, TEXT_SCALE) <= room {
                        self.list.text(x, label_y(last), &text, Tone::Ink100, TEXT_SCALE);
                    }
                }
                if let Some(fraction) = rendering {
                    self.draw_meter(self.rev_cell(), BORDER, REV, REV, fraction, false, false);
                }
                if self.feedback_shown() {
                    if self.worded() {
                        self.draw_control(self.feedback_cell(), BORDER, FEEDBACK, false, self.hover == Hover::Feedback, false);
                    } else {
                        self.draw_mark_cell(self.feedback_cell(), Mark::Heart, self.hover == Hover::Feedback, false);
                    }
                }
            }
            Screen::Settings => self.draw_control(close_cell(self.width), BORDER, "X", false, self.hover == Hover::Button(Button::Close), false),
        }
    }

    pub(super) fn draw_menu(&mut self, menu: Menu) {
        let rect = self.menu_rect(menu);
        let current = self.menu_current(menu);
        self.list.fill(rect, Tone::Ground);
        for (index, label) in self.menu_labels(menu).iter().enumerate() {
            let item = self.menu_item(menu, index);
            let hovered = self.menu == Some(menu) && self.hover == Hover::Item(index);
            if let Some(track) = self.delay_bar(menu, index) {
                self.draw_lean(item, label, self.project.tracks[track].delay_ms / DELAY_MS, hovered);
                continue;
            }
            let ink = if current == Some(index) {
                self.list.fill(item, Tone::Ink100);
                Tone::Ground
            } else if self.menu_inert(menu, index) {
                Tone::Ink50
            } else {
                if hovered {
                    self.list.fill(item, Tone::Ink25);
                }
                Tone::Ink100
            };
            self.list.text(item.x + TEXT_PAD, label_y(item), label, ink, TEXT_SCALE);
        }
        self.list.frame(rect, CONTROL_BORDER, Tone::Ink100);
    }
}

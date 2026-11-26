use super::*;
use super::automation::AUTO_LINE;
use super::draw_tracks::AUTO_NODE;
use super::view::return_name;
use super::wave::{ALL, FADE_INSET, FADE_NODES, SPAN_GRAB, ShownWave, SpanHit, SpanPart, fade_depth};

const LFO_MARK: [(i32, i32); 5] = [(1, 0), (0, 1), (2, 1), (4, 1), (3, 2)];
const LFO_MARK_W: i32 = 5;
const LFO_MARK_H: i32 = 3;
const HEAD_DESC: &str = "Where the clip starts in its file; STRT nudges it";
const FADE_STEPS: i32 = 4;
const FADE_SEAM: i32 = 1;

fn readings(audio: Option<&Audio>, host: Host, row: usize, deepest: f32) -> fx::Readings<'_> {
    fx::Readings {
        gauge: audio.map_or(0.0, |audio| audio.gauge(host, row)),
        deepest,
        signal: audio.map_or(f32::NEG_INFINITY, |audio| audio.signal(host, row)),
        strips: audio.map_or([[0.0; 2]; fx::params::FREAK_BANDS], |audio| audio.strips(host, row)),
        spectrum: audio.map(|audio| audio.spectrum(Scope::Insert)),
        cuts: audio.map_or([0.0; sdk::spectrum::CURVE], |audio| *audio.cuts(Scope::Insert)),
    }
}

impl App {
    pub(super) fn draw_automated_marks(&mut self, host: Host, row: usize) {
        let lanes = self.project.automation(host);
        let view = self.face.view();
        let rect_of = |aim: &Aim| match aim {
            Aim::Knob { row: at, band, name } if *at == row => self.project.rack(host)[row].faced().automated_rect(band, name, view),
            _ => None,
        };
        let mut rects: Vec<Rect> = Vec::new();
        for rect in lanes.curves.iter().filter(|curve| curve.lfo.is_none()).filter_map(|curve| rect_of(&curve.aim)) {
            if !rects.contains(&rect) {
                rects.push(rect);
            }
        }
        for rect in rects {
            let inner = face::interior(rect);
            face::draw_dot(&mut self.list, inner.x + inner.w - face::DOT_INSET - face::DOT, inner.y + face::DOT_INSET);
        }
        let mut tildes: Vec<Rect> = Vec::new();
        let waved = lanes.lfos.iter().filter(|lfo| lfo.amount > 0.0).filter_map(|lfo| rect_of(&lfo.aim));
        let curved = lanes.curves.iter().filter(|curve| curve.lfo.is_some()).filter_map(|curve| rect_of(&curve.aim));
        for rect in waved.chain(curved) {
            if !tildes.contains(&rect) {
                tildes.push(rect);
            }
        }
        for rect in tildes {
            let inner = face::interior(rect);
            let (x, y) = (inner.x + inner.w - face::DOT_INSET - LFO_MARK_W, inner.y + face::DOT_INSET);
            self.list.fill(Rect::new(x - 1, y - 1, LFO_MARK_W + 2, LFO_MARK_H + 2), Tone::Ground);
            for (dx, dy) in LFO_MARK {
                self.list.fill(Rect::new(x + dx, y + dy, 1, 1), Tone::Ink100);
            }
        }
    }

    pub(super) fn draw_palette(&mut self) {
        let sounding = self.under_playhead();
        let latched = self.live.latched.as_ref().map(|latch| latch.chord);
        let progression = self.edited_clip().map(|(track, index)| self.project.tracks[track].clips[index].chords()).unwrap_or_default();
        for deck in self.shown_decks() {
            let rect = self.palette_rect(deck);
            let (w, h) = ((rect.w as f32 / PALETTE_SCALE) as i32, (rect.h as f32 / PALETTE_SCALE) as i32);
            self.palettes[deck].layout(w, h, &self.project);
            self.list.clip(Some(rect));
            self.list.transform((rect.x, rect.y), PALETTE_SCALE);
            self.palettes[deck].draw(&mut self.list, &self.project, latched, sounding.as_ref(), &progression, self.view.palette_big);
            self.list.transform((0, 0), 1.0);
            self.list.clip(None);
        }
        if self.view.palette_big {
            let (upper, lower) = (self.palette_rect(1), self.palette_rect(0));
            self.list.fill(Rect::new(0, upper.y + upper.h, self.width, lower.y - upper.y - upper.h), Tone::Ink100);
        }
    }

    pub(super) fn draw_roll(&mut self) {
        self.roll.set_snap(self.timeline.snap.beats() as f32);
        let Some((track, index)) = self.edited_clip() else {
            return;
        };
        let rect = self.roll_rect();
        self.roll.layout(rect.w, rect.h);
        let real = &self.project.tracks[track].clips[index];
        let preview = self.take_preview(track);
        let clip = preview.as_ref().and_then(|preview| preview.iter().find(|clip| clip.start == real.start)).unwrap_or(real);
        let held = self.held_ghosts(track);
        self.roll.set_ghosts(self.ghosts_of(track, clip), held);
        let playhead = self.playing.then(|| (self.playhead - beats(clip.start)) as f32);
        let scale = scale_pitch_classes(self.project.root, self.project.scale);
        let beside = if self.roll.guide() { self.project.beside(track, clip) } else { Vec::new() };
        self.list.clip(Some(rect));
        self.list.transform((rect.x, rect.y), 1.0);
        self.roll.draw(&mut self.list, clip, &beside, playhead, &scale, self.menu == Some(Menu::Fit));
        self.list.transform((0, 0), 1.0);
        self.list.clip(None);
    }

    pub(super) fn draw_core(&mut self) {
        self.list.frame(self.core_cell(), BORDER, Tone::Ink100);
        self.list.frame(self.rack_cell(), BORDER, Tone::Ink100);
        self.list.frame(self.squares_cell(), BORDER, Tone::Ink100);
        let host = self.host();
        let shown = self.shown();
        let rows = self.readout_rows();
        let rect = self.face_rect();
        self.list.transform((rect.x, rect.y), 1.0);
        match shown {
            Some((host, row)) => {
                let (display, big) = (self.displayed(host, row), self.picture_big());
                let tv = self.tv_shown();
                let sound = host.track().map(|track| &self.project.tracks[track]);
                let idle = |list: &mut DrawList, _: Rect| {
                    if let Some(track) = sound {
                        face::show_sound(list, rows, &track.sound, &track.sound_desc);
                    }
                };
                match &display {
                    Row::Midi(midi) => midi.draw(&self.face, &mut self.list, idle),
                    Row::Core(layer) => layer.patch.draw(&self.face, &mut self.list, tv.map(|shown| (&self.tv, shown)), idle),
                    Row::Effect(slot) if big => slot.draw_plain(&self.face, &mut self.list),
                    Row::Effect(slot) => slot.draw(&self.face, &mut self.list, &self.picture, readings(self.audio.as_ref(), host, row, self.ducks.deepest())),
                    Row::Bus(_) => {}
                }
                self.draw_automated_marks(host, row);
                if let Some(Drag::Auto { host, armed, drag, .. }) = &self.drag
                    && let Some((value, desc)) = self.armed_spelling(*host, armed, self.dragged_travel(*host, *drag, armed))
                {
                    let (band, param) = armed.aim.words();
                    let (band, param) = (String::from(band), String::from(param));
                    match armed.lfo {
                        Some(field) => face::show_readout(&mut self.list, rows, &format!("{band} {param}"), &format!("LFO {}", field.label()), &value, desc),
                        None => face::show_readout(&mut self.list, rows, &band, &param, &value, desc),
                    }
                }
                if let Some(lane) = self.lfo_strip
                    && (matches!(self.hover, Hover::Lfo(_)) || self.lfo_face.dragging() || self.lfo_face.panel_open())
                    && let Some((host, armed)) = self.automations().get(lane).filter(|(_, armed)| armed.lfo.is_none()).cloned()
                    && let Some(index) = self.lfo_face.dragged_cell().or_else(|| self.lfo_face.hovered_cell())
                {
                    let entry = self.lane_lfo(host, &armed);
                    let cell = &lfo_band(&entry)[index];
                    face::show_readout(&mut self.list, rows, cell.param.band, cell.param.name, &face::spelled(cell, &entry), cell.desc);
                }
            }
            None => self.face.draw(&mut self.list, rows, &face::EMPTY, &(), |_, _| {}),
        }
        if let Some(index) = self.wave_face.dragged_cell().or_else(|| self.wave_face.nudged_cell()).or_else(|| self.wave_face.hovered_cell())
            && let Some(file) = self.shown_file().cloned()
        {
            let cell = &FILE_BAND[index];
            face::show_readout(&mut self.list, rows, cell.param.band, cell.param.name, &face::spelled(cell, &file), cell.desc);
        }
        if (matches!(self.drag, Some(Drag::Head)) || self.hover == Hover::WaveHead)
            && let Some(entry) = self.shown_file().map(File::entry)
        {
            face::show_readout(&mut self.list, rows, AUDIO, "HEAD", &clock(entry), HEAD_DESC);
        }
        if let Some((param, value, desc)) = self.wave_readout() {
            face::show_readout(&mut self.list, rows, AUDIO, param, &value, desc);
        }
        self.draw_square_readout();
        if let Some(notice) = &self.notice {
            let inner = face::interior(face::readout_rect(rows));
            self.list.clip(Some(Rect::new(rect.x + inner.x, rect.y + inner.y, inner.w, inner.h)));
            face::show_readout(&mut self.list, rows, notice.band, &notice.name, notice.reason, "");
            self.list.clip(None);
        }
        self.list.transform((0, 0), 1.0);
        let (rect, offer) = (self.rack_rect(), self.offer());
        self.list.transform((rect.x, rect.y), 1.0);
        let solo = host.track().and_then(|track| self.generator(track).core).map(|solo| (solo.row, solo.effects));
        let curved: Vec<bool> = self.project.rack(host).iter().enumerate().map(|(row, entry)| entry.mix_aim(row).is_some_and(|mixed| self.curve_exists(host, &mixed))).collect();
        self.rack.draw(&mut self.list, self.project.rack(host), shown.map(|(_, row)| row), offer, solo, &curved);
        self.list.transform((0, 0), 1.0);
        let (browser, save, load, spectrum) = (self.browser_square(), self.save_cell(), self.load_cell(), self.spectrum_square());
        self.draw_square(browser, "F", true, self.view.aside == Aside::Browser, self.hover == Hover::Button(Button::Show(Aside::Browser)));
        if self.stock_offered() {
            self.draw_square(self.stock_square(), "E", true, self.stock.is_some(), self.hover == Hover::Button(Button::Stock));
        }
        self.draw_square(save, "S", shown.is_some(), self.save.is_some(), self.hover == Hover::Button(Button::Save));
        self.draw_square(load, "L", true, false, self.hover == Hover::Load);
        self.draw_square(spectrum, "S", true, self.view.aside == Aside::Spectrum, self.hover == Hover::Button(Button::Show(Aside::Spectrum)));
        if self.wave_offered() {
            self.draw_square(self.wave_square(), "A", true, self.view.aside == Aside::Wave, self.hover == Hover::Button(Button::Show(Aside::Wave)));
        }
        if let Some((label, aside)) = self.own_button() {
            self.draw_square(self.own_square(), label, true, self.view.aside == aside, self.hover == Hover::Button(Button::Show(aside)));
        }
    }

    fn displayed(&self, host: Host, row: usize) -> Row {
        if self.face.dragged_cell().is_some() || self.tv.dragging() { self.project.rack(host)[row] } else { self.automated_row(host, row) }
    }

    pub(super) fn draw_big(&mut self) {
        let Some((host, row)) = self.shown().filter(|_| self.picture_big()) else {
            return;
        };
        let (rect, inner) = (self.face_rect(), self.picture_inner());
        if let Row::Effect(slot) = self.displayed(host, row) {
            self.list.transform((rect.x, rect.y), 1.0);
            slot.draw_big(&mut self.list, inner, &self.picture, readings(self.audio.as_ref(), host, row, self.ducks.deepest()));
            self.list.transform((0, 0), 1.0);
        }
    }

    pub(super) fn draw_square(&mut self, rect: Rect, label: &str, live: bool, on: bool, hovered: bool) {
        if live {
            face::draw_word_cell(&mut self.list, rect, label, on, hovered);
        } else {
            self.list.frame(rect, face::FRAME, Tone::Ink100);
            self.list.text_centered(face::interior(rect), label, Tone::Ink50, face::LABEL_SCALE);
        }
    }

    pub(super) fn draw_browser(&mut self) {
        let (rect, big) = (self.lay_browser(), self.browser_big());
        self.list.transform((rect.x, rect.y), 1.0);
        self.browser.draw(&mut self.list, self.pool.auditioned(), big);
        self.list.transform((0, 0), 1.0);
    }

    pub(super) fn draw_lanes(&mut self) {
        let Some((host, row)) = self.shown_midi() else {
            return;
        };
        let rect = self.lanes_rect();
        let playing = self.audio.as_ref().map(|audio| audio.signal(host, row)).filter(|step| *step >= 1.0).map(|step| step as i64 - 1);
        self.list.transform((rect.x, rect.y), 1.0);
        if let Row::Midi(midi) = &self.project.rack(host)[row] {
            self.lanes.layout(rect.w, &midi.arp);
            self.lanes.draw(&mut self.list, &midi.arp, playing);
        }
        self.list.transform((0, 0), 1.0);
    }

    pub(super) fn draw_wave(&mut self) {
        let (body, lane, row, band) = (self.wave_body(), self.wave_lane(), self.wave_row(), self.wave_band());
        let Some(shown) = self.shown_wave() else {
            return;
        };
        let Some(file) = self.shown_file().cloned() else {
            return;
        };
        let spans = self.shown_spans(&file);
        let half = body.h / 2;
        let middle = body.y + half;
        let rate = shown.wave.rate;
        let selected = shown.selection.map(|(from, to)| (shown.x_at(from * rate).clamp(0, body.w), shown.x_at(to * rate).clamp(0, body.w)));
        if let Some((x0, x1)) = selected {
            self.list.fill(Rect::new(body.x + x0, body.y, x1 - x0, body.h), Tone::Ink25);
        }
        let (played_from, played_to) = shown.played();
        let slopes = Slope::BOTH.map(|slope| file.fades.of(slope).map(|fade| (slope, fade, fade.ends(slope))));
        let line_y = |gain: f64| body.y + FADE_INSET + (AUTO_NODE - AUTO_LINE) / 2 + ((1.0 - gain) * f64::from(fade_depth(body))).round() as i32;
        for column in 0..body.w {
            let from = shown.frame_at(column);
            let to = from + shown.per_column;
            let peak = shown.wave.peak(from, to);
            let at = (from + to) / 2.0 / rate;
            let level = level_among(&spans, at) * file.fades.gain(at);
            let (full, h) = (grid::wave_height(peak, shown.gain, half), grid::wave_height(peak, shown.gain * level, half));
            let played = to > played_from && from < played_to;
            if played && full > h {
                self.list.fill(Rect::new(body.x + column, middle - full, 1, full - h), Tone::Ink50);
                self.list.fill(Rect::new(body.x + column, middle + h, 1, full - h), Tone::Ink50);
            }
            let inside = selected.is_some_and(|(x0, x1)| (x0..x1).contains(&column));
            let tone = match (played, inside) {
                (true, _) => Tone::Ink100,
                (false, true) => Tone::Ink50,
                (false, false) => Tone::Ink25,
            };
            let sounding = Rect::new(body.x + column, middle - h, 1, 2 * h);
            self.list.fill(sounding, tone);
            for (slope, fade, _) in slopes.into_iter().flatten().filter(|(_, _, (start, end))| to > start * rate && from < end * rate) {
                let ys = (0..=FADE_STEPS).map(|step| line_y(fade.gain(slope, (from + shown.per_column * f64::from(step) / f64::from(FADE_STEPS)) / rate)));
                let (top, bottom) = ys.fold((i32::MAX, i32::MIN), |(top, bottom), y| (top.min(y), bottom.max(y)));
                let stroke = Rect::new(body.x + column, top, 1, bottom - top + AUTO_LINE);
                self.list.fill(stroke, Tone::Ink100);
                if played {
                    self.list.fill(stroke.intersect(sounding), Tone::Ground);
                }
            }
        }
        if let Some((from, to)) = shown.selection {
            for edge in [from, to] {
                let x = shown.x_at(edge * rate);
                if (0..body.w).contains(&x) {
                    self.list.fill(Rect::new(body.x + x, body.y, 1, body.h), Tone::Ink100);
                }
            }
        }
        let line = |list: &mut DrawList, frame: f64, tone: Tone| {
            let x = shown.x_at(frame);
            if (0..=body.w - grid::PLAYHEAD_W).contains(&x) {
                list.fill(Rect::new(body.x + x, body.y, grid::PLAYHEAD_W, body.h), tone);
            }
        };
        if self.playing {
            let at = self.playhead - shown.start;
            let frame = shown.entry_frame() + at * shown.frames_per_beat * shown.ratio;
            if (0.0..shown.length).contains(&at) && frame < shown.stop {
                line(&mut self.list, frame, Tone::Ink50);
            }
        }
        line(&mut self.list, shown.entry_frame(), Tone::Ink100);
        line(&mut self.list, shown.stop, Tone::Ink100);
        let held = self.fade_held();
        for (slope, part) in FADE_NODES {
            let node = self.fade_node(&shown, slope, part);
            let shared = node == self.fade_node(&shown, slope.other(), part);
            let holding = held.filter(|&(_, at)| at == part).map(|(of, _)| of);
            let grown = if holding == Some(slope) && !shared { FADE_INSET } else { 0 };
            let around = |rect: Rect, by: i32| Rect::new(rect.x - by, rect.y - by, rect.w + 2 * by, rect.h + 2 * by).intersect(body);
            self.list.fill(around(node, grown + FADE_INSET), Tone::Ground);
            self.list.fill(around(node, grown), Tone::Ink100);
            if shared {
                let half = (AUTO_NODE - FADE_SEAM) / 2;
                self.list.fill(Rect::new(node.x + half, node.y, FADE_SEAM, node.h).intersect(body), Tone::Ground);
                if let Some(of) = holding {
                    let resting = if of == Slope::Rise { node.x + half + FADE_SEAM } else { node.x };
                    self.list.fill(Rect::new(resting, node.y, half, node.h).intersect(body), Tone::Ink50);
                }
            }
        }
        self.draw_span_lane(&shown, &spans, lane);
        let inner = face::interior(row);
        if matches!(self.drag, Some(Drag::Head)) || self.hover == Hover::WaveHead {
            self.list.fill(inner, Tone::Ink25);
        } else if shown.zoomed(body.w) {
            let (x0, x1) = (shown.x_whole(shown.left, inner.w), shown.x_whole(shown.left + f64::from(body.w) * shown.per_column, inner.w));
            self.list.fill(Rect::new(inner.x + x0, inner.y, (x1 - x0).max(1), inner.h), Tone::Ink25);
        }
        self.list.frame(row, face::FRAME, Tone::Ink100);
        let x = shown.x_whole(shown.entry_frame(), inner.w);
        if (0..=inner.w - grid::PLAYHEAD_W).contains(&x) {
            self.list.fill(Rect::new(inner.x + x, inner.y, grid::PLAYHEAD_W, inner.h), Tone::Ink100);
        }
        self.list.transform((band.x, band.y), 1.0);
        self.wave_face.draw_band(&mut self.list, &FILE_BAND, &file);
        self.list.transform((0, 0), 1.0);
        let (looped, reversed, saved, all) = (self.wave_loop_cell(), self.wave_reverse_cell(), self.wave_save_cell(), self.wave_all_cell());
        let rest = all.x + all.w - face::FRAME;
        self.list.frame(Rect::new(rest, band.y, band.x + band.w - rest, face::CELL), face::FRAME, Tone::Ink100);
        self.draw_wave_zoom(&shown, body.w);
        face::draw_word_cell(&mut self.list, looped, "LOOP", self.view.wave_loop, self.hover == Hover::WaveLoop);
        face::draw_word_cell(&mut self.list, reversed, "REV", false, self.hover == Hover::WaveReverse);
        face::draw_word_cell(&mut self.list, saved, "SAVE", false, self.hover == Hover::WaveSave);
        face::draw_word_cell(&mut self.list, all, ALL, false, self.hover == Hover::WaveAll);
    }

    fn draw_span_lane(&mut self, shown: &ShownWave, spans: &[Span], lane: Rect) {
        let inner = face::interior(lane);
        let (half, zero) = (inner.h / 2, inner.y + inner.h / 2);
        let rate = shown.wave.rate;
        let hovered = match self.hover {
            Hover::WaveLane(hit) => hit,
            _ => SpanHit::Nothing,
        };
        let dragged = match &self.drag {
            Some(Drag::Span(drag)) => Some((drag.index, drag.part)),
            _ => None,
        };
        let edges = |from: f64, to: f64| (shown.x_at(from * rate).clamp(0, inner.w), shown.x_at(to * rate).clamp(0, inner.w));
        self.list.frame(lane, face::FRAME, Tone::Ink100);
        if let Some((from, to)) = shown.selection {
            let (x0, x1) = edges(from, to);
            let offer = Rect::new(inner.x + x0, inner.y, x1 - x0, inner.h);
            if hovered == SpanHit::Offer {
                self.list.fill(offer, Tone::Ink25);
            }
            self.list.frame(offer, 1, Tone::Ink50);
        }
        self.list.fill(Rect::new(inner.x, zero, inner.w, 1), Tone::Ink25);
        for (index, span) in spans.iter().enumerate() {
            let (x0, x1) = edges(span.from, span.to);
            if x1 <= x0 {
                continue;
            }
            let extent = Rect::new(inner.x + x0, inner.y, x1 - x0, inner.h);
            let grab = SPAN_GRAB.min(extent.w / 3);
            let part = match (hovered, dragged) {
                (_, Some((held, part))) if held == index => Some(part),
                (SpanHit::Body(hit), None) if hit == index => Some(SpanPart::Level),
                (SpanHit::Start(hit), None) if hit == index => Some(SpanPart::Start),
                (SpanHit::End(hit), None) if hit == index => Some(SpanPart::End),
                _ => None,
            };
            match part {
                Some(SpanPart::Level) => self.list.fill(extent, Tone::Ink25),
                Some(SpanPart::Start) => self.list.fill(Rect::new(extent.x, inner.y, grab, inner.h), Tone::Ink25),
                Some(SpanPart::End) => self.list.fill(Rect::new(extent.x + extent.w - grab, inner.y, grab, inner.h), Tone::Ink25),
                None => {}
            }
            let h = ((span.level.abs() / File::DECIBELS * f64::from(half)).round() as i32).clamp(1, half.max(1));
            if span.level < 0.0 {
                self.list.fill(Rect::new(extent.x, zero, extent.w, h), Tone::Ink75);
                self.list.fill(Rect::new(extent.x, zero + h - 1, extent.w, 1), Tone::Ink100);
            } else if span.level > 0.0 {
                self.list.fill(Rect::new(extent.x, zero - h, extent.w, h), Tone::Ink75);
                self.list.fill(Rect::new(extent.x, zero - h, extent.w, 1), Tone::Ink100);
            }
            self.list.fill(Rect::new(extent.x, inner.y, 1, inner.h), Tone::Ink100);
            self.list.fill(Rect::new(extent.x + extent.w - 1, inner.y, 1, inner.h), Tone::Ink100);
            if shown.picked == Some(index) {
                self.list.frame(extent, face::FRAME, Tone::Ink100);
            }
        }
    }

    fn draw_wave_zoom(&mut self, shown: &ShownWave, w: i32) {
        let cell = self.wave_zoom_cell();
        let inner = face::interior(cell);
        let dragging = matches!(self.drag, Some(Drag::WaveZoom(_) | Drag::WaveZoomAt(_)));
        self.list.frame(cell, face::FRAME, Tone::Ink100);
        self.list.fill(inner, if dragging || self.hover == Hover::WaveZoom { Tone::Ink25 } else { Tone::Ground });
        let filled = inner.x + (inner.w as f32 * shown.zoom_travel(w)).round() as i32;
        self.list.fill(Rect::new(inner.x, inner.y, filled - inner.x, inner.h), Tone::Ink100);
        let label = if dragging { face::fit(&face::spell(Format::Seconds, shown.shown_seconds(w)), inner.w) } else { String::from("ZOOM") };
        let x = inner.x + (inner.w - font::width(&label, face::LABEL_SCALE)) / 2;
        let y = inner.y + (inner.h - font::GLYPH_H * face::LABEL_SCALE) / 2;
        self.list.text_split(x, y, &label, filled, Tone::Ground, Tone::Ink100, face::LABEL_SCALE);
    }

    pub(super) fn draw_spectrum(&mut self) {
        let inner = tile_inner(self.aside_cell());
        if let Some(bars) = self.audio.as_ref().map(|audio| audio.spectrum(Scope::Output)).filter(|spectrum| spectrum.live()) {
            spectrum::draw(&mut self.list, inner, bars.post(), Tone::Ink25);
        }
        if let Hover::Hertz(column) = self.hover {
            let cell = self.hertz_cell();
            self.list.fill(cell, Tone::Ground);
            self.draw_control(cell, BORDER, &face::spell(Format::Hertz, spectrum::hz(inner.w, column)), false, false, false);
        }
    }

    pub(super) fn draw_master(&mut self) {
        let host = self.panel_host();
        let label = if matches!(self.drag, Some(Drag::Master(_))) { decibels(self.master_gain()) } else { return_name(host) };
        self.draw_control(self.master_name_cell(), BORDER, &label, false, self.hover == Hover::Button(Button::MasterName), self.monitored.is_some());
        let cell = self.master_volume();
        self.list.frame(cell, BORDER, Tone::Ink100);
        let inner = tile_inner(cell);
        if matches!(self.drag, Some(Drag::Master(_))) || self.hover == Hover::Master {
            self.list.fill(inner, Tone::Ink25);
        }
        let (level, clip) = match (&self.audio, self.monitored) {
            (Some(audio), Some(Host::Send(send))) => (audio.send_level(send), (false, false)),
            (Some(audio), _) => (audio.master_level(), audio.clip()),
            (None, _) => ((0.0, 0.0), (false, false)),
        };
        self.draw_levels(inner, level, clip, CONTROL_BORDER);
        if self.monitored_send().is_none() {
            self.draw_send_levels(inner);
        }
        self.draw_handle(inner, self.automated_gain(host));
        if self.curve_exists(host, &Aim::Gain) {
            face::draw_dot(&mut self.list, inner.x + inner.w - face::DOT_INSET - face::DOT, inner.y + face::DOT_INSET);
        }
        self.draw_sends();
    }

    fn draw_send_levels(&mut self, inner: Rect) {
        for slot in 0..SENDS_PER_BANK {
            let send = self.send_ordinal(slot);
            let (left, right) = self.audio.as_ref().map_or((0.0, 0.0), |audio| audio.send_level(send));
            let level = left.max(right);
            if level <= 0.0 || self.soloed.is_some_and(|solo| solo != send) {
                continue;
            }
            let letter = tile_inner(self.send_cell(slot));
            let w = font::width(SEND_NAMES[send], TEXT_SCALE);
            let h = level_h(inner, level);
            let bar = Rect::new(letter.x + (letter.w - w) / 2, inner.y + inner.h - h, w, h);
            self.list.fill(Rect::new(bar.x - CONTROL_BORDER, bar.y - CONTROL_BORDER, bar.w + 2 * CONTROL_BORDER, bar.h + CONTROL_BORDER).intersect(inner), Tone::Ground);
            self.list.fill(bar, Tone::Ink75);
        }
    }

    pub(super) fn draw_sends(&mut self) {
        let lit: [bool; SENDS_PER_BANK] = std::array::from_fn(|slot| self.send_lit(self.send_ordinal(slot)));
        for slot in 0..SENDS_PER_BANK {
            let send = self.send_ordinal(slot);
            let cell = self.send_cell(slot);
            let inner = tile_inner(cell);
            let solo = self.soloed == Some(send);
            let on = self.send_lit(send);
            if on {
                self.list.fill(inner, Tone::Ink100);
            } else if self.hover == Hover::Button(Button::Send(slot)) {
                self.list.fill(inner, Tone::Ink25);
            }
            self.list.frame(cell, BORDER, Tone::Ink100);
            if solo {
                let ring = Rect::new(inner.x + CONTROL_BORDER, inner.y + CONTROL_BORDER, inner.w - 2 * CONTROL_BORDER, inner.h - 2 * CONTROL_BORDER);
                self.list.frame(ring, CONTROL_BORDER, Tone::Ground);
            }
            let ink = if on {
                Tone::Ground
            } else if self.send_fed(send) {
                Tone::Ink100
            } else {
                Tone::Ink50
            };
            self.list.text_centered(inner, SEND_NAMES[send], ink, TEXT_SCALE);
        }
        let cell = self.cycle_cell();
        let inner = tile_inner(cell);
        let hidden = |send: Option<usize>| send.is_some_and(|send| send / SENDS_PER_BANK != self.view.bank);
        let lit_elsewhere = hidden(self.monitored_send()) || hidden(self.soloed);
        if lit_elsewhere {
            self.list.fill(inner, Tone::Ink100);
        } else if self.hover == Hover::Button(Button::Cycle) {
            self.list.fill(inner, Tone::Ink25);
        }
        self.list.frame(cell, BORDER, Tone::Ink100);
        self.draw_mark(inner, Mark::Cycle, if lit_elsewhere { Tone::Ground } else { Tone::Ink100 });
        for slot in 0..SENDS_PER_BANK {
            if lit[slot] && slot > 0 && lit[slot - 1] {
                self.list.divide(self.send_cell(slot - 1), self.send_cell(slot), BORDER);
            }
            if lit[slot] && lit_elsewhere {
                self.list.divide(self.send_cell(slot), cell, BORDER);
            }
        }
    }
}

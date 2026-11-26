use super::*;
use super::automation::AUTO_GRAB;
use super::draw_tracks::{AUTO_HALO, AUTO_NODE};

const SPAN_PX_PER_DB: f64 = 5.0;
const SPAN_STEP: f64 = 0.1;
const SPAN_COARSE: f64 = 1.0;
const ZOOM_DESC: &str = "Down zooms in, up out, sideways pans; SHIFT-press fits";
const SELECT_DESC: &str = "Drag across to select; the lane below sets its level";
const SPAN_DESC: &str = "Drag sets its level; SHIFT-press or Delete removes it";
const EDGE_DESC: &str = "Drag to move this end; it cuts what it lands on";
const OFFER_DESC: &str = "Press and drag down to make a span here, up to lift it";
const LANE_DESC: &str = "Select a stretch above, then drag here to set its level";
const LOOP_DESC: &str = "Plays the selection round and round, the ruler's loop without one; SHIFT-press turns it off";
const REVERSE_DESC: &str = "Turns the clip's sound back to front, onto a file of its own; press again to turn it back";
const SAVE_DESC: &str = "Writes the clip as heard to a .wav file of your choosing";
const REVERSED: &str = " reverse";
const ALL_DESC: &str = "Gives this clip's fades, spans, PTCH and FIN to every clip on its track that sounds like it";
const ALIKE_SECONDS: f64 = 1.0;
const ALIKE_LENGTHS: f64 = 4.0;
pub(super) const ALL: &str = "ALL";
const IN_START_DESC: &str = "Drag across: silent up to here; SHIFT-press returns it to the start";
const IN_END_DESC: &str = "Drag across: the level rises from the lower node to here; SHIFT-press removes the fade";
const IN_BEND_DESC: &str = "Up lifts the level sooner, down holds it back longer; SHIFT double-press straightens it";
const OUT_START_DESC: &str = "Drag across: the level falls from here to the lower node; SHIFT-press removes the fade";
const OUT_END_DESC: &str = "Drag across: silent from here on; SHIFT-press returns it to the end";
const OUT_BEND_DESC: &str = "Up holds the level longer, down drops it sooner; SHIFT double-press straightens it";
const FADE_SELECT_DESC: &str = "Drag across to select; SHIFT-drag up or down bends the fade";

pub(super) const WAVE_LEAST_PER_COLUMN: f64 = 16.0;
pub(super) const SPAN_GRAB: i32 = 8;
pub(super) const FADE_INSET: i32 = (AUTO_HALO - AUTO_NODE) / 2;
pub(super) const FADE_NODES: [(Slope, FadePart); 4] = [(Slope::Rise, FadePart::Bottom), (Slope::Rise, FadePart::Top), (Slope::Fall, FadePart::Top), (Slope::Fall, FadePart::Bottom)];

pub(super) fn fade_depth(body: Rect) -> i32 {
    (body.h - AUTO_NODE - 2 * FADE_INSET).max(1)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum SpanHit {
    Body(usize),
    Start(usize),
    End(usize),
    Offer,
    Nothing,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum SpanPart {
    Level,
    Start,
    End,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum FadePart {
    Top,
    Bottom,
}

#[derive(Clone, Copy)]
pub(super) enum FadeDrag {
    Node { slope: Slope, part: FadePart, origin_x: i32, armed: bool },
    Bend { slope: Slope, at: f64, level: f64, y: i32 },
}

#[derive(Clone, Copy)]
pub(super) struct WaveZoomDrag {
    pub(super) origin: (i32, i32),
    pub(super) at_x: i32,
    pub(super) per_column: f64,
    pub(super) centre: f64,
}

#[derive(Clone, Copy)]
pub(super) struct WaveZoomAt {
    pub(super) origin: (i32, i32),
    pub(super) per_column: f64,
    pub(super) frame: f64,
}

#[derive(Clone, Copy)]
pub(super) struct SelectDrag {
    pub(super) origin_x: i32,
    pub(super) at_x: i32,
    pub(super) armed: bool,
}

#[derive(Clone, Copy)]
pub(super) struct SpanDrag {
    pub(super) index: usize,
    pub(super) part: SpanPart,
    origin: (i32, i32),
    span: Span,
    laid: Span,
}

pub(super) struct WaveView {
    name: String,
    left: f64,
    per_column: f64,
    selection: Option<(f64, f64)>,
    picked: Option<usize>,
}

pub(super) struct ShownWave {
    pub(super) wave: Arc<Wave>,
    entry: f64,
    pub(super) ratio: f64,
    pub(super) gain: f64,
    pub(super) fades: Fades,
    pub(super) stop: f64,
    pub(super) start: f64,
    pub(super) length: f64,
    pub(super) frames_per_beat: f64,
    pub(super) left: f64,
    pub(super) per_column: f64,
    pub(super) selection: Option<(f64, f64)>,
    pub(super) picked: Option<usize>,
}

impl ShownWave {
    pub(super) fn entry_frame(&self) -> f64 {
        self.entry * self.wave.rate
    }

    pub(super) fn played(&self) -> (f64, f64) {
        let from = self.entry_frame();
        (from, (from + self.length * self.frames_per_beat * self.ratio).min(self.stop))
    }

    fn frames(&self) -> f64 {
        self.wave.frames() as f64
    }

    pub(super) fn x_at(&self, frame: f64) -> i32 {
        ((frame - self.left) / self.per_column).round() as i32
    }

    pub(super) fn frame_at(&self, x: i32) -> f64 {
        self.left + f64::from(x) * self.per_column
    }

    fn beat_at(&self, seconds: f64) -> f64 {
        self.start + (seconds * self.wave.rate - self.entry_frame()) / (self.frames_per_beat * self.ratio)
    }

    fn selected_beats(&self) -> Option<(f64, f64)> {
        let (from, to) = self.selection?;
        Some((self.beat_at(from), self.beat_at(to)))
    }

    fn seconds_at_x(&self, x: i32) -> f64 {
        (self.frame_at(x) / self.wave.rate).clamp(0.0, self.wave.seconds())
    }

    fn fade_rest(&self, slope: Slope) -> f64 {
        let start = self.entry.clamp(0.0, self.wave.seconds());
        match slope {
            Slope::Rise => start,
            Slope::Fall => (self.played().1 / self.wave.rate).clamp(start, self.wave.seconds()),
        }
    }

    fn reaching(&self, slope: Slope, fade: Option<Fade>) -> Option<Fade> {
        let rest = self.fade_rest(slope);
        fade.filter(|fade| match slope {
            Slope::Rise => fade.to > rest,
            Slope::Fall => fade.from < rest,
        })
    }

    fn fade_ends(&self, slope: Slope) -> (f64, f64) {
        let rest = self.fade_rest(slope);
        self.fades.of(slope).map_or((rest, rest), |fade| fade.ends(slope))
    }

    fn fade_seconds(&self, slope: Slope, part: FadePart) -> f64 {
        let (from, to) = self.fade_ends(slope);
        match (slope, part) {
            (Slope::Rise, FadePart::Bottom) | (Slope::Fall, FadePart::Top) => from,
            (Slope::Rise, FadePart::Top) | (Slope::Fall, FadePart::Bottom) => to,
        }
    }

    fn fade_at(&self, x: i32) -> Option<(Slope, Fade, f64)> {
        let seconds = self.frame_at(x) / self.wave.rate;
        Slope::BOTH.into_iter().find_map(|slope| {
            let fade = self.fades.of(slope)?;
            let (start, end) = fade.ends(slope);
            (seconds > start && seconds < end).then_some((slope, fade, seconds))
        })
    }

    fn span_x(&self, span: Span) -> (i32, i32) {
        (self.x_at(span.from * self.wave.rate), self.x_at(span.to * self.wave.rate))
    }

    pub(super) fn x_whole(&self, frame: f64, w: i32) -> i32 {
        (frame / self.frames() * f64::from(w)).round() as i32
    }

    fn seconds_at(&self, x: i32, w: i32) -> f64 {
        (f64::from(x) / f64::from(w) * self.wave.seconds()).clamp(0.0, self.wave.seconds())
    }

    fn fit(&self, w: i32) -> f64 {
        self.frames() / f64::from(w.max(1))
    }

    fn least(&self, w: i32) -> f64 {
        WAVE_LEAST_PER_COLUMN.min(self.fit(w))
    }

    pub(super) fn zoomed(&self, w: i32) -> bool {
        self.per_column < self.fit(w)
    }

    pub(super) fn zoom_travel(&self, w: i32) -> f32 {
        let (fit, least) = (self.fit(w), self.least(w));
        if least >= fit {
            return 0.0;
        }
        ((fit / self.per_column).log2() / (fit / least).log2()).clamp(0.0, 1.0) as f32
    }

    pub(super) fn shown_seconds(&self, w: i32) -> f64 {
        f64::from(w) * self.per_column / self.wave.rate
    }
}

fn stepped(level: f64) -> f64 {
    ((level / SPAN_STEP).round() * SPAN_STEP).clamp(-File::DECIBELS, File::DECIBELS)
}

impl App {
    pub(super) fn shown_clip(&self) -> Option<(usize, usize)> {
        if let Some(&(track, start)) = self.selection.clips.last()
            && let Some(owner) = self.project.tracks.get(track).filter(|owner| owner.audio)
            && let Some(index) = owner.clips.iter().position(|clip| clip.start == start)
        {
            return Some((track, index));
        }
        let track = self.project.tracks.get(self.selection.track).filter(|track| track.audio)?;
        (!track.clips.is_empty()).then_some((self.selection.track, 0))
    }

    pub(super) fn shown_file(&self) -> Option<&File> {
        let (track, index) = self.shown_clip()?;
        self.project.tracks[track].clips[index].file.as_ref()
    }

    fn shown_file_mut(&mut self) -> Option<&mut File> {
        let (track, index) = self.shown_clip()?;
        self.project.tracks[track].clips[index].file.as_mut()
    }

    pub(super) fn shown_wave(&self) -> Option<ShownWave> {
        let (track, index) = self.shown_clip()?;
        let clip = &self.project.tracks[track].clips[index];
        let file = clip.file.as_ref()?;
        let wave = self.pool.wave_of(clip).filter(|wave| wave.frames() > 0)?;
        let frames_per_beat = wave.rate * 60.0 / self.project.bpm;
        let w = f64::from(self.wave_body().w.max(1));
        let fit = wave.frames() as f64 / w;
        let (left, per_column, selection, picked) = match &self.wave_view {
            Some(view) if view.name == wave.name => {
                let per_column = view.per_column.clamp(WAVE_LEAST_PER_COLUMN.min(fit), fit);
                (view.left.clamp(0.0, (wave.frames() as f64 - w * per_column).max(0.0)), per_column, view.selection, view.picked)
            }
            _ => (0.0, fit, None, None),
        };
        Some(ShownWave { stop: file.stop(wave.frames(), wave.rate), wave, entry: file.entry(), ratio: file.stretch(self.project.bpm), gain: file.gain(), fades: file.fades, start: beats(clip.start), length: beats(clip.length), frames_per_beat, left, per_column, selection, picked })
    }

    fn wave_view_mut(&mut self) -> Option<&mut WaveView> {
        let shown = self.shown_wave()?;
        if self.wave_view.as_ref().is_none_or(|view| view.name != shown.wave.name) {
            self.wave_view = Some(WaveView { name: shown.wave.name.clone(), left: shown.left, per_column: shown.per_column, selection: None, picked: None });
        }
        self.wave_view.as_mut()
    }

    fn view_wave(&mut self, left: f64, per_column: f64) -> bool {
        let w = self.wave_body().w;
        let Some(shown) = self.shown_wave() else {
            return false;
        };
        let per_column = per_column.clamp(shown.least(w), shown.fit(w));
        let left = left.clamp(0.0, (shown.frames() - f64::from(w) * per_column).max(0.0));
        let Some(view) = self.wave_view_mut() else {
            return false;
        };
        let changed = view.left != left || view.per_column != per_column;
        view.left = left;
        view.per_column = per_column;
        changed
    }

    pub(super) fn zoom_wave(&mut self, per_column: f64, centre: f64) -> bool {
        let w = self.wave_body().w;
        let Some(shown) = self.shown_wave() else {
            return false;
        };
        let per_column = per_column.clamp(shown.least(w), shown.fit(w));
        self.view_wave(centre - f64::from(w) / 2.0 * per_column, per_column)
    }

    fn wave_centre(&self, shown: &ShownWave) -> f64 {
        match shown.selection {
            Some((from, to)) => (from + to) / 2.0 * shown.wave.rate,
            None => shown.left + f64::from(self.wave_body().w) / 2.0 * shown.per_column,
        }
    }

    pub(super) fn wave_zoom_down(&mut self, x: i32, y: i32, reset: bool) -> bool {
        let Some(shown) = self.shown_wave() else {
            return false;
        };
        if reset || self.shift {
            let fit = shown.fit(self.wave_body().w);
            return self.view_wave(0.0, fit);
        }
        let centre = self.wave_centre(&shown);
        self.drag = Some(Drag::WaveZoom(WaveZoomDrag { origin: (x, y), at_x: x, per_column: shown.per_column, centre }));
        true
    }

    pub(super) fn wave_zoom_at(&mut self, x: i32, y: i32) -> bool {
        let Some(shown) = self.shown_wave() else {
            return false;
        };
        let frame = shown.frame_at(x - self.wave_body().x);
        self.drag = Some(Drag::WaveZoomAt(WaveZoomAt { origin: (x, y), per_column: shown.per_column, frame }));
        true
    }

    pub(super) fn zoom_wave_under(&mut self, per_column: f64, frame: f64, x: i32) -> bool {
        let body = self.wave_body();
        let Some(shown) = self.shown_wave() else {
            return false;
        };
        let per_column = per_column.clamp(shown.least(body.w), shown.fit(body.w));
        self.view_wave(frame - f64::from(x - body.x) * per_column, per_column)
    }

    pub(super) fn throttle_wave_pan(&mut self, seconds: f64) -> bool {
        let Some(Drag::WaveZoom(mut drag)) = self.drag else {
            return false;
        };
        let dx = drag.at_x - drag.origin.0;
        if dx.abs() < DRAG_ARM_PX {
            return false;
        }
        let Some(shown) = self.shown_wave() else {
            return false;
        };
        let half = f64::from(self.wave_body().w) / 2.0 * shown.per_column;
        drag.centre = (drag.centre + f64::from(dx * dx.abs()) / grid::PAN_DAMPING * seconds * shown.per_column).clamp(half, (shown.frames() - half).max(half));
        self.drag = Some(Drag::WaveZoom(drag));
        self.zoom_wave(shown.per_column, drag.centre)
    }

    pub(super) fn wave_wheel(&mut self, x: i32, dx: i32, dy: i32) -> bool {
        let Some(shown) = self.shown_wave() else {
            return false;
        };
        let under = shown.frame_at(x - self.wave_body().x);
        self.zoom_wave_under(shown.per_column / (f64::from(dy) / grid::ZOOM_PX_PER_DOUBLE).exp2(), under, x + dx)
    }

    pub(super) fn body_down(&mut self, x: i32, y: i32, reset: bool) -> bool {
        let unpicked = self.unpick_span();
        let under = self.shown_wave().and_then(|shown| shown.fade_at(x - self.wave_body().x));
        if let Some((slope, fade, at)) = under.filter(|_| self.shift) {
            if reset {
                return unpicked | self.set_fade(slope, Some(Fade { bend: 0.0, ..fade }));
            }
            self.drag = Some(Drag::Fade(FadeDrag::Bend { slope, at, level: fade.gain(slope, at), y }));
            return true;
        }
        self.drag = Some(Drag::Select(SelectDrag { origin_x: x, at_x: x, armed: false }));
        unpicked
    }

    pub(super) fn fade_node(&self, shown: &ShownWave, slope: Slope, part: FadePart) -> Rect {
        let body = self.wave_body();
        let seconds = shown.fade_seconds(slope, part);
        let y = match part {
            FadePart::Top => body.y + FADE_INSET,
            FadePart::Bottom => body.y + FADE_INSET + fade_depth(body),
        };
        let (left, x) = (body.x + FADE_INSET, body.x + shown.x_at(seconds.min(shown.wave.seconds()) * shown.wave.rate) - AUTO_NODE / 2);
        Rect::new(x.clamp(left, (body.x + body.w - FADE_INSET - AUTO_NODE).max(left)), y, AUTO_NODE, AUTO_NODE)
    }

    fn fade_hit(&self, shown: &ShownWave, x: i32, y: i32) -> Option<(Slope, FadePart)> {
        let body = self.wave_body();
        let nodes = FADE_NODES.map(|(slope, part)| (slope, part, self.fade_node(shown, slope, part)));
        let reached = |node: &Rect| Rect::new(node.x - AUTO_GRAB, node.y - AUTO_GRAB, node.w + 2 * AUTO_GRAB, node.h + 2 * AUTO_GRAB).intersect(body).contains(x, y);
        let middle = |node: &Rect| node.x + AUTO_NODE / 2;
        let other_half = |slope: Slope, node: &Rect| (slope == Slope::Fall) != (x > middle(node));
        let nearest = nodes.into_iter().filter(|(_, _, node)| reached(node)).min_by_key(|(slope, _, node)| ((x - middle(node)).abs(), other_half(*slope, node)));
        nearest.map(|(slope, part, _)| (slope, part))
    }

    pub(super) fn fade_held(&self) -> Option<(Slope, FadePart)> {
        match (&self.drag, self.hover) {
            (Some(Drag::Fade(FadeDrag::Node { slope, part, .. })), _) => Some((*slope, *part)),
            (None, Hover::WaveFade(slope, part)) => Some((slope, part)),
            _ => None,
        }
    }

    pub(super) fn fade_down(&mut self, slope: Slope, part: FadePart, x: i32, reset: bool) -> bool {
        self.unpick_span();
        if reset || self.shift {
            return self.reset_fade(slope, part);
        }
        self.drag = Some(Drag::Fade(FadeDrag::Node { slope, part, origin_x: x, armed: false }));
        true
    }

    fn reset_fade(&mut self, slope: Slope, part: FadePart) -> bool {
        let Some(shown) = self.shown_wave() else {
            return false;
        };
        let rest = shown.fade_rest(slope);
        let reaching = shown.reaching(slope, shown.fades.of(slope));
        let fade = match (part, slope) {
            (FadePart::Top, _) => None,
            (FadePart::Bottom, Slope::Rise) => reaching.map(|rise| Fade { from: rest, ..rise }),
            (FadePart::Bottom, Slope::Fall) => reaching.map(|fall| Fade { to: rest, ..fall }),
        };
        self.set_fade(slope, fade)
    }

    fn set_fade(&mut self, slope: Slope, fade: Option<Fade>) -> bool {
        let Some((track, index)) = self.shown_clip() else {
            return false;
        };
        let shown = &self.project.tracks[track].clips[index];
        let Some(entry) = shown.file.as_ref().map(File::entry) else {
            return false;
        };
        let (start, until) = (shown.start, self.heard_until(shown));
        let mut changed = false;
        for &(track, start) in self.selection.clips.iter().chain([&(track, start)]) {
            let heard = self.project.tracks.get(track).and_then(|owner| owner.clips.iter().find(|clip| clip.start == start)).and_then(|clip| self.heard_until(clip));
            let file = self.project.tracks.get_mut(track).and_then(|track| track.clips.iter_mut().find(|clip| clip.start == start)).and_then(|clip| clip.file.as_mut());
            if let Some(file) = file {
                let by_start = file.entry() - entry;
                let by_end = heard.zip(until).map_or(by_start, |(theirs, ours)| theirs - ours);
                let moved = file.fades.with(slope, fade.map(|fade| fade.carried(slope, by_start, by_end, file.entry())));
                changed |= file.fades != moved;
                file.fades = moved;
            }
        }
        changed
    }

    pub(super) fn fade_moved(&mut self, drag: FadeDrag, x: i32, y: i32) -> bool {
        let Some(shown) = self.shown_wave() else {
            return false;
        };
        let body = self.wave_body();
        match drag {
            FadeDrag::Node { slope, part, origin_x, armed } => {
                let arming = !armed && (x - origin_x).abs() >= DRAG_ARM_PX;
                let stacked = self.fade_node(&shown, Slope::Rise, part) == self.fade_node(&shown, Slope::Fall, part);
                let seconds = shown.seconds_at_x(x - body.x);
                let slope = if !(arming && stacked) {
                    slope
                } else if seconds < shown.fade_seconds(Slope::Rise, part) {
                    Slope::Rise
                } else if seconds > shown.fade_seconds(Slope::Fall, part) {
                    Slope::Fall
                } else {
                    slope
                };
                let armed = armed || arming;
                self.drag = Some(Drag::Fade(FadeDrag::Node { slope, part, origin_x, armed }));
                if !armed {
                    return false;
                }
                let rest = shown.fade_rest(slope);
                let held = shown.reaching(slope, shown.fades.of(slope)).unwrap_or(Fade { from: rest, to: rest, bend: 0.0 });
                let moved = match (slope, part) {
                    (Slope::Rise, FadePart::Bottom) | (Slope::Fall, FadePart::Top) => Fade { from: seconds.min(held.to), ..held },
                    (Slope::Rise, FadePart::Top) | (Slope::Fall, FadePart::Bottom) => Fade { to: seconds.max(held.from), ..held },
                };
                self.set_fade(slope, shown.reaching(slope, shown.fades.with(slope, Some(moved)).of(slope)))
            }
            FadeDrag::Bend { slope, at, level, y: pressed } => {
                let Some(fade) = shown.fades.of(slope) else {
                    return false;
                };
                let ((start, end), (before, after)) = (fade.ends(slope), slope.levels());
                let through = level + f64::from(pressed - y) / f64::from(fade_depth(body));
                self.set_fade(slope, Some(Fade { bend: bend_through(before, after, (at - start) / (end - start), through), ..fade }))
            }
        }
    }

    pub(super) fn select_wave(&mut self, a: i32, b: i32) -> bool {
        let Some(shown) = self.shown_wave() else {
            return false;
        };
        let x = self.wave_body().x;
        let (from, to) = (shown.seconds_at_x(a.min(b) - x), shown.seconds_at_x(a.max(b) - x));
        let selection = (to - from >= Span::LEAST).then_some((from, to));
        let Some(view) = self.wave_view_mut() else {
            return false;
        };
        let changed = view.selection != selection;
        view.selection = selection;
        if changed && self.view.wave_loop {
            self.enter_wave_loop();
        }
        changed
    }

    pub(super) fn wave_loop_beats(&self) -> Option<(f64, f64)> {
        if !self.view.wave_loop || self.view.aside != Aside::Wave {
            return None;
        }
        let (start, end) = self.shown_wave()?.selected_beats()?;
        let start = start.max(0.0);
        (end > start).then_some((start, end))
    }

    pub(super) fn toggle_wave_loop(&mut self, reset: bool) -> bool {
        self.view.wave_loop = !reset && !self.shift && !self.view.wave_loop;
        if self.view.wave_loop {
            self.enter_wave_loop();
        }
        true
    }

    fn enter_wave_loop(&mut self) {
        let Some((start, end)) = self.wave_loop_beats() else {
            return;
        };
        let at = if self.playing { self.playhead } else { beats(self.play_from) };
        if !(start..end).contains(&at) {
            self.place_playhead(ticks(start));
        }
    }


    pub(super) fn reverse_clip(&mut self) -> bool {
        let Some((track, index)) = self.shown_clip() else {
            return false;
        };
        let clip = self.project.tracks[track].clips[index].clone();
        let Some(file) = clip.file.clone() else {
            return false;
        };
        match self.bounced(std::slice::from_ref(&clip), clip.start, clip.end(), |first| first.strip_suffix(REVERSED).map_or_else(|| format!("{first}{REVERSED}"), String::from)) {
            Ok((wave, level)) => {
                let name = wave.name.clone();
                self.pool.keep(wave.reversed(), None);
                self.project.tracks[track].clips[index].file = Some(File { level, ..File::new(name) });
                self.wave_view = None;
                true
            }
            Err(reason) => {
                self.notice = Some(Notice { band: AUDIO, name: file.name, reason });
                false
            }
        }
    }

    pub(super) fn save_clip_wave(&mut self) -> bool {
        let Some((track, index)) = self.shown_clip() else {
            return false;
        };
        let clip = self.project.tracks[track].clips[index].clone();
        let Some(file) = clip.file.clone() else {
            return false;
        };
        match self.bounced(std::slice::from_ref(&clip), clip.start, clip.end(), str::to_owned) {
            Ok((wave, _)) => {
                self.requests.push(Request::Wave { name: format!("{}.wav", file.name), bytes: wave.wav() });
                true
            }
            Err(reason) => {
                self.notice = Some(Notice { band: AUDIO, name: file.name, reason });
                false
            }
        }
    }

    fn sounded(&self, clip: &Clip) -> Option<(Arc<Wave>, f64, f64)> {
        let file = clip.file.as_ref()?;
        let wave = self.pool.wave_of(clip).filter(|wave| wave.frames() > 0)?;
        let bpm = self.project.bpm;
        let (from, stop) = (file.entry().max(0.0), file.stop(wave.frames(), wave.rate) / wave.rate);
        let until = (file.entry() + beats(clip.length) * 60.0 / bpm * file.stretch(bpm)).min(stop);
        Some((wave, from, (until - from).max(0.0)))
    }

    pub(super) fn heard_until(&self, clip: &Clip) -> Option<f64> {
        self.sounded(clip).map(|(_, from, seconds)| from + seconds)
    }

    fn alike(&self, clip: &Clip, other: &Clip) -> bool {
        let (Some((wave, from, seconds)), Some((theirs, start, length))) = (self.sounded(clip), self.sounded(other)) else {
            return false;
        };
        let shared = seconds.min(length).min(ALIKE_SECONDS);
        seconds.max(length) <= ALIKE_LENGTHS * seconds.min(length) && wave.likeness(from, shared).alike(&theirs.likeness(start, shared))
    }

    pub(super) fn alike_among(&self, clips: &[Clip], like: usize) -> Vec<usize> {
        (0..clips.len()).filter(|&at| at != like && self.alike(&clips[like], &clips[at])).collect()
    }

    pub(super) fn match_clips(&mut self) -> bool {
        let Some((track, index)) = self.shown_clip() else {
            return false;
        };
        let clips = &self.project.tracks[track].clips;
        if clips[index].file.is_none() {
            return false;
        }
        let alike = self.alike_among(clips, index);
        let matched: Vec<Option<File>> = alike.iter().map(|&at| clips[at].matched(&clips[index], |clip| self.heard_until(clip))).collect();
        let owner = &mut self.project.tracks[track];
        for (&at, file) in alike.iter().zip(matched) {
            owner.clips[at].file = file;
        }
        let name = owner.name.clone();
        self.notice = Some(if alike.is_empty() { Notice { band: ALL, name, reason: "NO OTHERS" } } else { Notice { band: ALL, name: format!("{} ON {name}", counted(alike.len() as i32, "CLIP")), reason: "MATCHED" } });
        true
    }

    pub(super) fn clear_wave_selection(&mut self) -> bool {
        self.wave_view.as_mut().is_some_and(|view| view.selection.take().is_some())
    }

    pub(super) fn wave_escape(&mut self) -> bool {
        let Some(view) = self.wave_view.as_mut() else {
            return false;
        };
        view.picked.take().is_some() | view.selection.take().is_some()
    }

    pub(super) fn lane_hit(&self, shown: &ShownWave, x: i32) -> SpanHit {
        let x = x - face::interior(self.wave_lane()).x;
        if let Some((from, to)) = shown.selection
            && (shown.x_at(from * shown.wave.rate)..shown.x_at(to * shown.wave.rate)).contains(&x)
        {
            return SpanHit::Offer;
        }
        if let Some(file) = self.shown_file() {
            for (index, span) in file.spans.iter().enumerate() {
                let (x0, x1) = shown.span_x(*span);
                if x < x0 || x >= x1 {
                    continue;
                }
                let grab = SPAN_GRAB.min((x1 - x0) / 3);
                return if x < x0 + grab {
                    SpanHit::Start(index)
                } else if x >= x1 - grab {
                    SpanHit::End(index)
                } else {
                    SpanHit::Body(index)
                };
            }
        }
        SpanHit::Nothing
    }

    pub(super) fn lane_down(&mut self, hit: SpanHit, x: i32, y: i32) -> bool {
        match hit {
            SpanHit::Body(index) if self.shift => self.remove_span(index),
            SpanHit::Body(index) => self.grab_span(index, SpanPart::Level, x, y),
            SpanHit::Start(index) => self.grab_span(index, SpanPart::Start, x, y),
            SpanHit::End(index) => self.grab_span(index, SpanPart::End, x, y),
            SpanHit::Offer => {
                let Some((from, to)) = self.shown_wave().and_then(|shown| shown.selection) else {
                    return false;
                };
                let Some(file) = self.shown_file_mut() else {
                    return false;
                };
                let index = file.lay_span(Span { from, to, level: 0.0 });
                self.clear_wave_selection();
                self.grab_span(index, SpanPart::Level, x, y)
            }
            SpanHit::Nothing => self.unpick_span(),
        }
    }

    fn grab_span(&mut self, index: usize, part: SpanPart, x: i32, y: i32) -> bool {
        let Some(span) = self.shown_file().and_then(|file| file.spans.get(index).copied()) else {
            return false;
        };
        self.face.unpick();
        self.rack.unpick();
        self.wave_face.unpick();
        self.drag = Some(Drag::Span(SpanDrag { index, part, origin: (x, y), span, laid: span }));
        self.pick_span(Some(index));
        true
    }

    pub(super) fn unpick_span(&mut self) -> bool {
        self.wave_view.as_mut().is_some_and(|view| view.picked.take().is_some())
    }

    fn pick_span(&mut self, index: Option<usize>) -> bool {
        let Some(view) = self.wave_view_mut() else {
            return false;
        };
        let changed = view.picked != index;
        view.picked = index;
        changed
    }

    pub(super) fn span_moved(&mut self, drag: SpanDrag, x: i32, y: i32) -> bool {
        let Some(shown) = self.shown_wave() else {
            return false;
        };
        let seconds = shown.seconds_at_x(x - self.wave_body().x);
        let laid = match drag.part {
            SpanPart::Level => Span { level: stepped(drag.span.level + f64::from(drag.origin.1 - y) / SPAN_PX_PER_DB), ..drag.laid },
            SpanPart::Start => Span { from: seconds.min(drag.laid.to - Span::LEAST).max(0.0), ..drag.laid },
            SpanPart::End => Span { to: seconds.max(drag.laid.from + Span::LEAST).min(shown.wave.seconds()), ..drag.laid },
        };
        let changed = laid != drag.laid;
        self.drag = Some(Drag::Span(SpanDrag { laid, ..drag }));
        if drag.part == SpanPart::Level
            && let Some(span) = self.shown_file_mut().and_then(|file| file.spans.get_mut(drag.index))
        {
            span.level = laid.level;
        }
        changed
    }

    pub(super) fn span_released(&mut self, drag: SpanDrag) {
        if drag.part == SpanPart::Level {
            return;
        }
        let Some(file) = self.shown_file_mut() else {
            return;
        };
        if drag.index < file.spans.len() {
            file.spans.remove(drag.index);
        }
        let index = file.lay_span(drag.laid);
        self.pick_span(Some(index));
    }

    pub(super) fn shown_spans(&self, file: &File) -> Vec<Span> {
        let mut spans = file.spans.clone();
        if let Some(Drag::Span(drag)) = &self.drag
            && let Some(span) = spans.get_mut(drag.index)
        {
            *span = drag.laid;
        }
        spans
    }

    fn remove_span(&mut self, index: usize) -> bool {
        let Some(file) = self.shown_file_mut().filter(|file| index < file.spans.len()) else {
            return false;
        };
        file.spans.remove(index);
        self.unpick_span();
        true
    }

    fn picked_span(&self) -> Option<usize> {
        if self.view.aside != Aside::Wave || !self.core_shown() {
            return None;
        }
        let index = self.shown_wave()?.picked?;
        self.shown_file().filter(|file| index < file.spans.len()).map(|_| index)
    }

    pub(super) fn delete_picked_span(&mut self) -> bool {
        self.picked_span().is_some_and(|index| self.remove_span(index))
    }

    pub(super) fn nudge_span(&mut self, key: Key, shift: bool) -> bool {
        let Some((index, direction)) = self.picked_span().zip(key.vertical()) else {
            return false;
        };
        let step = if shift { SPAN_COARSE } else { SPAN_STEP };
        let Some(span) = self.shown_file_mut().and_then(|file| file.spans.get_mut(index)) else {
            return false;
        };
        let level = stepped(span.level + f64::from(direction) * step);
        let changed = span.level != level;
        span.level = level;
        changed
    }

    pub(super) fn wave_readout(&self) -> Option<(&'static str, String, &'static str)> {
        let shown = self.shown_wave()?;
        let w = self.wave_body().w;
        let selected = || shown.selection.map(|(from, to)| face::spell(Format::Seconds, to - from));
        if let Some((slope, part)) = self.fade_held() {
            let (name, desc) = match (slope, part) {
                (Slope::Rise, FadePart::Bottom) => ("FADE IN START", IN_START_DESC),
                (Slope::Rise, FadePart::Top) => ("FADE IN END", IN_END_DESC),
                (Slope::Fall, FadePart::Top) => ("FADE OUT START", OUT_START_DESC),
                (Slope::Fall, FadePart::Bottom) => ("FADE OUT END", OUT_END_DESC),
            };
            return Some((name, clock(shown.fade_seconds(slope, part)), desc));
        }
        Some(match (&self.drag, self.hover) {
            (Some(Drag::Fade(FadeDrag::Bend { slope, .. })), _) => {
                let bend = face::spell(Format::SignedPercent, f64::from(shown.fades.of(*slope).map_or(0.0, |fade| fade.bend)));
                match slope {
                    Slope::Rise => ("FADE IN BEND", bend, IN_BEND_DESC),
                    Slope::Fall => ("FADE OUT BEND", bend, OUT_BEND_DESC),
                }
            }
            (Some(Drag::WaveZoom(_) | Drag::WaveZoomAt(_)), _) | (None, Hover::WaveZoom) => ("ZOOM", face::spell(Format::Seconds, shown.shown_seconds(w)), ZOOM_DESC),
            (None, Hover::WaveLoop) => ("LOOP", String::from(face::OFF_ON[usize::from(self.view.wave_loop)]), LOOP_DESC),
            (None, Hover::WaveReverse) => ("REVERSE", shown.wave.name.clone(), REVERSE_DESC),
            (None, Hover::WaveSave) => ("SAVE", shown.wave.name.clone(), SAVE_DESC),
            (None, Hover::WaveAll) => (ALL, shown.wave.name.clone(), ALL_DESC),
            (Some(Drag::Select(drag)), _) => ("SELECT", selected().unwrap_or_else(|| clock(shown.seconds_at_x(drag.at_x - self.wave_body().x))), SELECT_DESC),
            (None, Hover::WaveBody(x)) => ("SELECT", selected().unwrap_or_else(|| clock(shown.seconds_at_x(x))), if shown.fade_at(x).is_some() { FADE_SELECT_DESC } else { SELECT_DESC }),
            (Some(Drag::Span(drag)), _) => match drag.part {
                SpanPart::Level => ("SPAN", face::spell(Format::Decibels, drag.laid.level), SPAN_DESC),
                SpanPart::Start => ("SPAN START", clock(drag.laid.from), EDGE_DESC),
                SpanPart::End => ("SPAN END", clock(drag.laid.to), EDGE_DESC),
            },
            (None, Hover::WaveLane(hit)) => {
                let file = self.shown_file()?;
                match hit {
                    SpanHit::Body(index) => ("SPAN", face::spell(Format::Decibels, file.spans.get(index)?.level), SPAN_DESC),
                    SpanHit::Start(index) => ("SPAN START", clock(file.spans.get(index)?.from), EDGE_DESC),
                    SpanHit::End(index) => ("SPAN END", clock(file.spans.get(index)?.to), EDGE_DESC),
                    SpanHit::Offer => ("SPAN", selected().unwrap_or_default(), OFFER_DESC),
                    SpanHit::Nothing => ("SPANS", counted(file.spans.len() as i32, "SPAN"), LANE_DESC),
                }
            }
            _ => return None,
        })
    }

    fn with_file(&mut self, gesture: impl FnOnce(&mut Face, &mut File) -> bool) -> bool {
        let Some((track, index)) = self.shown_clip() else {
            return false;
        };
        let Some(file) = self.project.tracks[track].clips[index].file.as_mut() else {
            return false;
        };
        let ratio = file.ratio();
        let before = FILE_BAND.map(|cell| (cell.param.get)(file));
        let changed = gesture(&mut self.wave_face, file);
        let after = FILE_BAND.map(|cell| (cell.param.get)(file));
        let retuned = if file.warp { 1.0 } else { file.ratio() / ratio };
        let mut spread = false;
        for (cell, (was, now)) in before.into_iter().zip(after).enumerate() {
            if was != now {
                spread |= self.spread(cell, now);
            }
        }
        let followed = self.settings.bpm_follows_pitch && retuned != 1.0 && self.scale_tempo(retuned);
        changed | spread | followed
    }

    fn spread(&mut self, index: usize, value: f64) -> bool {
        let cell = &FILE_BAND[index];
        let bpm = self.project.bpm;
        let mut changed = false;
        for &(track, start) in &self.selection.clips {
            let file = self.project.tracks.get_mut(track).and_then(|track| track.clips.iter_mut().find(|clip| clip.start == start)).and_then(|clip| clip.file.as_mut());
            if let Some(file) = file
                && (cell.param.get)(file) != value
            {
                (cell.param.set)(file, value);
                if let Some(tempo) = cell.tempo {
                    tempo(file, bpm);
                }
                changed = true;
            }
        }
        changed
    }

    pub(super) fn wave_down(&mut self, x: i32, y: i32) -> bool {
        let unpicked = self.unpick_span();
        let (px, py) = self.wave_point(x, y);
        let bpm = self.project.bpm;
        if self.shift {
            return unpicked | self.wave_face.hovered_cell().is_some_and(|index| self.default_cell(index));
        }
        unpicked | self.with_file(|wave_face, file| wave_face.down(px, py, face::NO_READOUT, &FILE_BAND, file, bpm))
    }

    pub(super) fn wave_moved(&mut self, x: i32, y: i32) -> bool {
        let (px, py) = self.wave_point(x, y);
        self.with_file(|wave_face, file| wave_face.moved(px, py, face::NO_READOUT, &FILE_BAND, file))
    }

    pub(super) fn wave_up(&mut self, x: i32, y: i32) -> bool {
        if !self.wave_face.dragging() && !self.wave_face.panel_open() {
            return false;
        }
        let (px, py) = self.wave_point(x, y);
        self.with_file(|wave_face, file| wave_face.up(px, py, face::NO_READOUT, &FILE_BAND, file))
    }

    fn default_cell(&mut self, index: usize) -> bool {
        let cell = &FILE_BAND[index];
        let value = (cell.param.get)(&File::new(String::new()));
        self.with_file(|_, file| {
            let changed = (cell.param.get)(file) != value;
            (cell.param.set)(file, value);
            changed
        }) | self.spread(index, value)
    }

    fn picked_file_cell(&self) -> Option<usize> {
        self.wave_face.picked().filter(|_| self.view.aside == Aside::Wave && self.core_shown())
    }

    pub(super) fn delete_picked_file_cell(&mut self) -> bool {
        self.picked_file_cell().is_some_and(|index| self.default_cell(index))
    }

    pub(super) fn nudge_file(&mut self, key: Key, shift: bool) -> bool {
        let Some(direction) = key.vertical().filter(|_| self.picked_file_cell().is_some()) else {
            return false;
        };
        let bpm = self.project.bpm;
        self.with_file(|wave_face, file| wave_face.nudge(&FILE_BAND, file, direction, shift, bpm))
    }

    pub(super) fn head_down(&mut self, x: i32) -> bool {
        if self.shift {
            return self.set_head(0.0);
        }
        self.drag = Some(Drag::Head);
        self.head_to(x)
    }

    pub(super) fn head_to(&mut self, x: i32) -> bool {
        let Some(shown) = self.shown_wave() else {
            return false;
        };
        let inner = face::interior(self.wave_row());
        let nudge = self.shown_file().map_or(0.0, |file| file.offset);
        self.set_head(shown.seconds_at(x - inner.x, inner.w) - nudge)
    }

    fn set_head(&mut self, head: f64) -> bool {
        let Some(file) = self.shown_file_mut() else {
            return false;
        };
        let changed = file.head != head;
        file.head = head;
        changed
    }

    pub(super) fn wave_hover(&mut self, x: i32, y: i32) -> bool {
        let point = (self.hover == Hover::Wave || self.wave_face.dragging()).then(|| self.wave_point(x, y));
        self.wave_face.hover(point, face::NO_READOUT, &FILE_BAND)
    }

    pub(super) fn wave_hover_at(&self, x: i32, y: i32) -> Option<Hover> {
        if self.view.aside != Aside::Wave {
            return None;
        }
        if let Some(shown) = self.shown_wave() {
            if self.wave_zoom_cell().contains(x, y) {
                return Some(Hover::WaveZoom);
            }
            if self.wave_loop_cell().contains(x, y) {
                return Some(Hover::WaveLoop);
            }
            if self.wave_reverse_cell().contains(x, y) {
                return Some(Hover::WaveReverse);
            }
            if self.wave_save_cell().contains(x, y) {
                return Some(Hover::WaveSave);
            }
            if self.wave_all_cell().contains(x, y) {
                return Some(Hover::WaveAll);
            }
            if face::interior(self.wave_lane()).contains(x, y) {
                return Some(Hover::WaveLane(self.lane_hit(&shown, x)));
            }
            if face::interior(self.wave_row()).contains(x, y) {
                return Some(Hover::WaveHead);
            }
            if let Some((slope, part)) = self.fade_hit(&shown, x, y) {
                return Some(Hover::WaveFade(slope, part));
            }
            if self.wave_body().contains(x, y) {
                return Some(Hover::WaveBody(x - self.wave_body().x));
            }
        }
        (self.wave_band().contains(x, y) && self.shown_file().is_some()).then_some(Hover::Wave)
    }
}

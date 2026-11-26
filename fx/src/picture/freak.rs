use sdk::draw::{DrawList, Rect, Tone};
use sdk::face::Format::Hertz;
use sdk::face::{LABEL_SCALE, THROW, spell};
use sdk::font;
use crate::face::Cells;
use crate::params::freak::{FreakBand, SHAPE1, SHAPE2, SHAPE3, SHAPE4};
use crate::params::{FREAK_BANDS, Freak};
use sdk::param::Param;
use sdk::spectrum::{self, Spectrum};
use super::eq::{hz_of_x, x_of_hz};
use super::{Drag, FINE, GAP, GRAB, LABEL_PAD, Picture, plot};

const BAND_FULL_DB: f32 = 24.0;
const CORNER: i32 = 32;
const BUTTON: i32 = 22;
const BUTTON_SCALE: i32 = 2;

pub(super) type Crossovers = [f64; FREAK_BANDS - 1];
pub(super) type Collapsed = [bool; FREAK_BANDS];

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum BandState {
    On,
    Solo,
    Mute,
    Off,
}

impl BandState {
    pub(super) fn of(on: bool, solo: bool, mute: bool) -> BandState {
        match (on, solo, mute) {
            (false, _, _) => BandState::Off,
            (true, true, _) => BandState::Solo,
            (true, false, true) => BandState::Mute,
            (true, false, false) => BandState::On,
        }
    }

    pub(super) fn apply(self, on: &mut bool, solo: &mut bool, mute: &mut bool) {
        *on = self != BandState::Off;
        *solo = self == BandState::Solo;
        *mute = self == BandState::Mute;
    }

    fn next(self) -> BandState {
        match self {
            BandState::On => BandState::Solo,
            BandState::Solo => BandState::Mute,
            BandState::Mute => BandState::Off,
            BandState::Off => BandState::On,
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            BandState::On => "",
            BandState::Solo => "S",
            BandState::Mute => "M",
            BandState::Off => "O",
        }
    }
}

pub(crate) trait Banded: Cells {
    const INS: [Param<Self>; FREAK_BANDS];
    const OUTS: [Param<Self>; FREAK_BANDS];
    const MIN_HZ: f64;
    const MAX_HZ: f64;
    const RATIO: f64;
    fn count(&self) -> usize;
    fn crossovers(&self) -> Crossovers;
    fn set_crossovers(&mut self, hz: Crossovers);
    fn state(&self, band: usize) -> BandState;
    fn set_state(&mut self, band: usize, state: BandState);
    fn reducing(&self, band: usize) -> bool;
    fn output_db(&self, band: usize) -> f64;
    fn spelled(&self, band: usize) -> String {
        band_gain(self.output_db(band) as f32)
    }
    fn split(&mut self, band: usize, hz: f64) -> bool;
    fn destroy(&mut self, gone: Collapsed);
}

impl Banded for Freak {
    const INS: [Param<Freak>; FREAK_BANDS] = [SHAPE1.input, SHAPE2.input, SHAPE3.input, SHAPE4.input];
    const OUTS: [Param<Freak>; FREAK_BANDS] = [SHAPE1.output, SHAPE2.output, SHAPE3.output, SHAPE4.output];
    const MIN_HZ: f64 = Freak::MIN_CROSSOVER_HZ;
    const MAX_HZ: f64 = Freak::MAX_CROSSOVER_HZ;
    const RATIO: f64 = Freak::CROSSOVER_RATIO;

    fn count(&self) -> usize {
        Freak::count(self)
    }

    fn crossovers(&self) -> Crossovers {
        self.crossovers_hz
    }

    fn set_crossovers(&mut self, hz: Crossovers) {
        self.crossovers_hz = hz;
    }

    fn state(&self, band: usize) -> BandState {
        BandState::of(self.bands[band].on, self.bands[band].solo, self.bands[band].mute)
    }

    fn set_state(&mut self, band: usize, state: BandState) {
        let band = &mut self.bands[band];
        state.apply(&mut band.on, &mut band.solo, &mut band.mute);
    }

    fn reducing(&self, band: usize) -> bool {
        self.bands[band].squeezing()
    }

    fn output_db(&self, band: usize) -> f64 {
        self.bands[band].output_db
    }

    fn split(&mut self, band: usize, hz: f64) -> bool {
        split_bands(&mut self.bands, &mut self.crossovers_hz, &mut self.band_count, band, hz, (Self::MIN_HZ, Self::MAX_HZ, Self::RATIO))
    }

    fn destroy(&mut self, gone: Collapsed) {
        destroy_bands(&mut self.bands, &mut self.band_count, gone, FreakBand::DEFAULT);
    }
}

pub(super) fn split_bands<B: Copy>(bands: &mut [B; FREAK_BANDS], crossovers: &mut Crossovers, band_count: &mut usize, band: usize, hz: f64, (floor, ceiling, ratio): (f64, f64, f64)) -> bool {
    let count = (*band_count).clamp(1, FREAK_BANDS);
    if count >= FREAK_BANDS {
        return false;
    }
    let low = if band == 0 { floor } else { crossovers[band - 1] * ratio };
    let high = if band + 1 == count { ceiling } else { crossovers[band] / ratio };
    if low > high {
        return false;
    }
    bands.copy_within(band..count, band + 1);
    crossovers.copy_within(band..count - 1, band + 1);
    crossovers[band] = hz.clamp(low, high);
    *band_count = count + 1;
    true
}

pub(super) fn destroy_bands<B: Copy>(bands: &mut [B; FREAK_BANDS], band_count: &mut usize, gone: Collapsed, rest: B) {
    let count = (*band_count).clamp(1, FREAK_BANDS);
    let survivors = bands.iter().zip(gone).take(count).filter(|(_, gone)| !gone).map(|(settings, _)| *settings);
    let mut kept = [rest; FREAK_BANDS];
    for (slot, settings) in kept.iter_mut().zip(survivors) {
        *slot = settings;
    }
    *bands = kept;
    *band_count = count - gone.iter().filter(|gone| **gone).count();
}

pub(crate) struct Marks {
    selected: Option<usize>,
    collapse: Option<(usize, Collapsed)>,
}

fn band_gain(gain_db: f32) -> String {
    if gain_db.abs() < 0.05 { String::from("0.0 dB") } else { format!("{gain_db:+.1} dB") }
}

fn squeezed(split: usize, line: usize) -> usize {
    if line > split { line } else { line + 1 }
}

fn pushed(collapse: Option<(usize, Collapsed)>, line: usize) -> bool {
    collapse.is_some_and(|(split, collapsed)| line != split && collapsed[squeezed(split, line)])
}

fn collapsed(split: usize, before: Crossovers, hz: Crossovers) -> Collapsed {
    let mut collapsed = [false; FREAK_BANDS];
    for line in (0..FREAK_BANDS - 1).filter(|&line| line != split && hz[line] != before[line]) {
        collapsed[squeezed(split, line)] = true;
    }
    collapsed
}

fn bounds<P: Banded>(p: &P, band: usize) -> (f64, f64) {
    let crossovers = p.crossovers();
    let low = if band == 0 { P::MIN_HZ } else { crossovers[band - 1] };
    let high = if band + 1 == p.count() { P::MAX_HZ } else { crossovers[band] };
    (low, high)
}

fn splittable<P: Banded>(p: &P, band: usize) -> bool {
    let (low, high) = bounds(p, band);
    p.count() < FREAK_BANDS && low * P::RATIO <= high / P::RATIO
}

fn buttons<P: Banded>(p: &P, band: usize, start: i32, end: i32) -> (bool, bool) {
    let width = end - start;
    (width >= CORNER && p.count() > 1, width >= 2 * CORNER && splittable(p, band))
}

fn draw_button(list: &mut DrawList, x: i32, top: i32, glyph: &str) {
    let square = Rect::new(x, top, BUTTON, BUTTON);
    list.frame(square, 1, Tone::Ink100);
    list.text(x + (BUTTON - font::width(glyph, BUTTON_SCALE)) / 2, top + (BUTTON - font::GLYPH_H * BUTTON_SCALE) / 2, glyph, Tone::Ink100, BUTTON_SCALE);
}

fn band_at<P: Banded>(area: Rect, p: &P, x: i32) -> usize {
    p.crossovers()[..p.count() - 1].iter().filter(|hz| area.x + x_of_hz(**hz, area.w) <= x).count()
}

fn on_state_square<P: Banded>(area: Rect, p: &P, band: usize, x: i32, y: i32) -> bool {
    let edges = edges_of(area, p, None);
    let middle = area.y + area.h / 2;
    edges[band + 1] - edges[band] >= CORNER && (2 * x - edges[band] - edges[band + 1]).abs() <= BUTTON && (y - middle).abs() <= BUTTON / 2
}

fn edges_of<P: Banded>(area: Rect, p: &P, collapse: Option<(usize, Collapsed)>) -> [i32; FREAK_BANDS + 1] {
    let mut edges = [area.x + area.w; FREAK_BANDS + 1];
    edges[0] = area.x;
    for (edge, hz) in edges[1..p.count()].iter_mut().zip(p.crossovers()) {
        *edge = area.x + x_of_hz(hz, area.w);
    }
    if let Some((split, _)) = collapse {
        let at = edges[split + 1];
        for line in (0..p.count() - 1).filter(|&line| pushed(collapse, line)) {
            edges[line + 1] = at;
        }
    }
    edges
}

pub(crate) fn draw_banded<P: Banded>(list: &mut DrawList, inner: Rect, p: &P, strips: [[f32; 2]; FREAK_BANDS], spectrum: Option<&Spectrum>, marks: Marks, under: impl FnOnce(&mut DrawList, Rect)) {
    let area = plot(inner);
    let count = p.count();
    let edges = edges_of(area, p, marks.collapse);
    if let Some(band) = marks.selected.filter(|band| *band < count) {
        list.fill(Rect::new(edges[band], area.y, edges[band + 1] - edges[band], area.h), Tone::Ink25);
    }
    if let Some(spectrum) = spectrum.filter(|spectrum| spectrum.live()) {
        spectrum::draw(list, area, spectrum.pre(), Tone::Ink25);
        spectrum::draw(list, area, spectrum.post(), Tone::Ink50);
    }
    under(list, area);
    let middle = area.y + area.h / 2;
    let text_h = font::GLYPH_H * LABEL_SCALE;
    let full = area.h / 2 - LABEL_PAD - text_h;
    let collapsed = marks.collapse.map_or([false; FREAK_BANDS], |(_, collapsed)| collapsed);
    list.fill(Rect::new(area.x, middle, area.w, 1), Tone::Ink50);
    for (band, [reduction, _]) in strips.into_iter().enumerate().take(count).filter(|(band, _)| !collapsed[*band]) {
        let (start, end) = (edges[band], edges[band + 1]);
        let state = p.state(band);
        let on = state != BandState::Off;
        let reach = (f64::from(full) * f64::from((reduction / BAND_FULL_DB).clamp(0.0, 1.0))).round() as i32;
        if on && p.reducing(band) && reach > 0 {
            let bar = Rect::new(start + GAP, middle, (end - start - 2 * GAP).max(0), reach);
            list.fill(bar, Tone::Ink75);
            list.fill(Rect::new(bar.x, bar.y + bar.h - 1, bar.w, 1), Tone::Ink100);
        }
        let out_db = p.output_db(band) as f32;
        let lift = (f64::from(full) * f64::from((out_db / BAND_FULL_DB).clamp(-1.0, 1.0))).round() as i32;
        list.fill(Rect::new(start, middle - lift, (end - start).max(0), 1), Tone::Ink100);
        let ink = if on { Tone::Ink100 } else { Tone::Ink50 };
        list.text(start + LABEL_PAD, area.y + LABEL_PAD, &(band + 1).to_string(), ink, LABEL_SCALE);
        if end - start >= CORNER {
            let square = Rect::new((start + end - BUTTON) / 2, middle - BUTTON / 2, BUTTON, BUTTON);
            list.fill(square, Tone::Ground);
            draw_button(list, square.x, square.y, state.glyph());
        }
        let (spelled, room) = (p.spelled(band), font::glyphs_in(end - start - 2 * LABEL_PAD, LABEL_SCALE));
        let value = font::shortened(spelled.strip_suffix(" dB").filter(|_| spelled.chars().count() > room).unwrap_or(&spelled), room);
        let (minus, plus) = buttons(p, band, start, end);
        let top = area.y + area.h - LABEL_PAD - BUTTON;
        let beside = end - start >= font::width(&value, LABEL_SCALE) + if minus || plus { 2 * CORNER } else { 0 };
        let foot = if beside { top + (BUTTON - text_h) / 2 } else { top - GAP - text_h };
        list.text(start + (end - start - font::width(&value, LABEL_SCALE)) / 2, foot, &value, ink, LABEL_SCALE);
        if minus {
            draw_button(list, start + (CORNER - BUTTON) / 2, top, "-");
        }
        if plus {
            draw_button(list, end - (CORNER + BUTTON) / 2, top, "+");
        }
    }
    let past_number = |edge: i32| edge + 2 * LABEL_PAD + font::GLYPH_W * LABEL_SCALE;
    let mut left = past_number(area.x);
    for (_, (split, hz)) in edges[1..count].iter().zip(p.crossovers()).enumerate().filter(|(line, _)| !pushed(marks.collapse, *line)) {
        list.fill(Rect::new(*split, area.y, 1, area.h), Tone::Ink100);
        let label = font::shortened(&spell(Hertz, hz), font::glyphs_in(split - LABEL_PAD - left, LABEL_SCALE));
        list.text(split - LABEL_PAD - font::width(&label, LABEL_SCALE), area.y + LABEL_PAD, &label, Tone::Ink100, LABEL_SCALE);
        left = past_number(*split);
    }
}

impl Picture {
    pub(super) fn down_banded<P: Banded>(&mut self, x: i32, y: i32, inner: Rect, p: &mut P, clicks: i32) -> bool {
        let area = plot(inner);
        let crossovers = p.crossovers();
        let lines = &crossovers[..p.count() - 1];
        let hit = lines.iter().map(|hz| (area.x + x_of_hz(*hz, area.w) - x).abs()).enumerate().filter(|(_, distance)| *distance <= GRAB / 2).min_by_key(|(_, distance)| *distance);
        if let Some((split, _)) = hit {
            self.drag = Some(Drag::Crossover { split, before: crossovers });
            return true;
        }
        let mut band = band_at(area, p, x);
        let edges = edges_of(area, p, None);
        let (minus, plus) = buttons(p, band, edges[band], edges[band + 1]);
        if on_state_square(area, p, band, x, y) {
            p.set_state(band, p.state(band).next());
            self.band = Some(band);
            return true;
        }
        if y >= area.y + area.h - CORNER {
            if minus && x < edges[band] + CORNER {
                self.remove_band(band, p);
                return true;
            }
            if plus && x >= edges[band + 1] - CORNER {
                let (low, high) = bounds(p, band);
                if p.split(band, (low * high).sqrt()) {
                    self.band = Some(band + 1);
                }
                return true;
            }
        }
        if clicks == 2 && p.split(band, hz_of_x(x - area.x, area.w)) {
            band += 1;
        }
        self.band = Some(band);
        self.drag = Some(Drag::Gain { band, anchors: [P::OUTS[band].travel_of(p), P::INS[band].travel_of(p)] });
        true
    }

    pub(super) fn right_banded<P: Banded>(&mut self, x: i32, y: i32, inner: Rect, p: &mut P) -> bool {
        let area = plot(inner);
        let band = band_at(area, p, x);
        if !on_state_square(area, p, band, x, y) || p.state(band) == BandState::On {
            return false;
        }
        p.set_state(band, BandState::On);
        self.band = Some(band);
        true
    }

    fn remove_band<P: Banded>(&mut self, band: usize, p: &mut P) {
        let count = p.count();
        let line = band.saturating_sub(1);
        let mut hz = p.crossovers();
        hz.copy_within(line + 1..count - 1, line);
        let mut gone = [false; FREAK_BANDS];
        gone[band] = true;
        p.destroy(gone);
        p.set_crossovers(hz);
        self.band = Some(line);
    }

    pub(super) fn moved_crossover<P: Banded>(&self, x: i32, inner: Rect, fine: bool, p: &mut P) -> bool {
        let Some(Drag::Crossover { split, before }) = self.drag else {
            return false;
        };
        let Some(above) = (p.count() - 1).checked_sub(split + 1) else {
            return false;
        };
        let area = plot(inner);
        let ratio = P::RATIO;
        let mut hz = before;
        let at = if fine { area.x + x_of_hz(before[split], area.w) + (x - self.origin.0) / FINE } else { x };
        hz[split] = hz_of_x(at - area.x, area.w).clamp(P::MIN_HZ * ratio.powi(split as i32), P::MAX_HZ / ratio.powi(above as i32));
        for line in split + 1..split + 1 + above {
            hz[line] = before[line].max(hz[line - 1] * ratio).min(P::MAX_HZ);
        }
        for line in (0..split).rev() {
            hz[line] = before[line].min(hz[line + 1] / ratio).max(P::MIN_HZ);
        }
        p.set_crossovers(hz);
        true
    }

    pub(super) fn moved_gain<P: Banded>(&mut self, dy: i32, command: bool, fine: bool, p: &mut P) -> bool {
        let Some(Drag::Gain { band, anchors }) = self.drag else {
            return false;
        };
        let param = if command { P::INS[band] } else { P::OUTS[band] };
        let throw = if fine { THROW * f64::from(FINE) } else { THROW };
        param.set_travel(p, anchors[usize::from(command)] - f64::from(dy) / throw);
        true
    }

    pub(super) fn release_crossover<P: Banded>(&mut self, split: usize, before: Crossovers, p: &mut P) {
        let count = p.count();
        let gone = collapsed(split, before, p.crossovers());
        if !gone.contains(&true) {
            return;
        }
        let standing = p.crossovers().into_iter().enumerate().take(count - 1).filter(|(line, _)| !pushed(Some((split, gone)), *line)).map(|(_, hz)| hz);
        let mut hz = p.crossovers();
        for (slot, crossover) in hz.iter_mut().zip(standing) {
            *slot = crossover;
        }
        p.destroy(gone);
        p.set_crossovers(hz);
        self.band = self.band.map(|band| band.min(p.count() - 1));
    }

    pub(crate) fn marks<P: Banded>(&self, p: &P) -> Marks {
        let collapse = match self.drag {
            Some(Drag::Crossover { split, before }) => Some((split, collapsed(split, before, p.crossovers()))),
            _ => None,
        };
        Marks { selected: self.band, collapse }
    }
}

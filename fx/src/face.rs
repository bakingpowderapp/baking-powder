use sdk::division::{self, Division};
use sdk::draw::{DrawList, Rect};
use sdk::face::{self, Act, Cell, Face, Faced, Format::*, Kind::*, VALUE_SCALE, button, choice, choose, desc, field, flag, flat, held, inert, open, page, paged, pick, refuses, saved, seeded, switch, tempo, title, toggle, trayed, unless, value, viewed, wide, widened, View};
use crate::Params;
use sdk::keyboard::Key;
use sdk::param::{self, Param};
use crate::params::{self as params, FREAK_BANDS, Age, BandStereo, Channel, ChorusMode, Compressor, Curve, DelayMode, Delay, EarlyPattern, Eq, EqBand, Filtered, FlangerMode, FlangerShape, Freak, FreakSlope, Gate, Lookahead, LookaheadCurve, DetectMode, DetectSlope, DetectStereo, Motion, MotionMode, Pass, PhaserShape, Repeat, RepeatMix, Reverb, RippleShape, Routing, STAGES, STAGE_LABELS, Shape, Sidechain, Signal, Slope, Slot, Space, Suppressor, Tank, Time, Transient, Utility, Vary, VolumeLfo, EQ_BANDS, PASS_BANDS, FLIP_LABELS, Grid, INTERVALS, INTERVAL_LABELS};
use crate::picture::{draw_banded, draw_curve, draw_duck, draw_filter, draw_duck_tv, draw_ramp, draw_reduction, draw_shape, draw_space, draw_suppress};
use crate::picture::Picture;
use sdk::spectrum::{CURVE, Spectrum};

const DIVISIONS: &[&str] = &division::LABELS;

#[derive(Clone, Copy)]
pub struct Readings<'a> {
    pub gauge: f32,
    pub deepest: f32,
    pub signal: f32,
    pub strips: [[f32; 2]; FREAK_BANDS],
    pub spectrum: Option<&'a Spectrum>,
    pub cuts: [f32; CURVE],
}

macro_rules! filter_cells {
    ($tray:expr, $band:literal, $site:literal, $params:expr, $($unit:tt)+) => {
        trayed(
            [
                desc(toggle(-1, 0, "ON", $band, "ON", flag!($($unit)+.on)), "Filter on"),
                desc(switch(-1, 1, Pass::LABELS, $band, "TYPE", choose!(Pass::ALL, $($unit)+.pass)), "Low-pass, high-pass or band-pass"),
                desc(switch(-1, 2, Slope::LABELS, $band, "SLOPE", choose!(Slope::ALL, $($unit)+.slope)), "Slope in dB per octave"),
                desc(wide(value(Rise, -1, 3, "FREQ", $params.freq, Hertz), 2), concat!("Cutoff; filters ", $site)),
                desc(unless(toggle(-1, 5, "RES", $band, "RESONANT", flag!($($unit)+.resonant)), |p| p.$($unit)+.pass == Pass::Band || p.$($unit)+.slope == Slope::Six), "Resonant cut shaped by Q; needs 12 or 24"),
                desc(unless(value(Rise, -1, 6, "Q", $params.q, TwoPlaces), |p| !p.$($unit)+.peaked()), "Resonance, or band width; 0.71 is flat"),
            ],
            $tray,
        )
    };
}

macro_rules! source {
    ($row:literal, $about:literal) => {
        desc(pick($row, 2, 4, "SIGNAL", "SOURCE", field!(|p| p.signal.source, |p, v| p.signal.source = v.round()), &[]), $about)
    };
}

macro_rules! tap {
    ($row:literal, $span:literal) => {
        desc(wide(saved(seeded(switch($row, 6, &["POST", "PRE"], "SIGNAL", "TAP", flag!(signal.pre)), "KEY", "TAP"), Signal::PRE_EFFECTS), $span), "PRE before source effects; POST at source output")
    };
}

pub(crate) mod compressor;
pub(crate) mod delay;
pub(crate) mod eq;
pub(crate) mod freak;
pub(crate) mod gate;
pub(crate) mod motion;
pub(crate) mod repeat;
pub(crate) mod reverb;
pub(crate) mod sidechain;
pub(crate) mod space;
pub(crate) mod suppressor;
pub(crate) mod time;
pub(crate) mod transient;
pub(crate) mod utility;
pub(crate) mod volume_lfo;

pub(super) use eq::soloed;

macro_rules! each {
    ($params:expr, |$cells:ident, $p:ident| $body:expr) => {
        match $params {
            Params::Compressor($p) => { let $cells: &[Cell<Compressor>] = &compressor::CELLS; $body }
            Params::Delay($p) => { let $cells: &[Cell<Delay>] = &delay::CELLS; $body }
            Params::Eq($p) => { let $cells: &[Cell<Eq>] = &eq::CELLS; $body }
            Params::Freak($p) => { let $cells: &[Cell<Freak>] = &freak::CELLS; $body }
            Params::Gate($p) => { let $cells: &[Cell<Gate>] = &gate::CELLS; $body }
            Params::Motion($p) => { let $cells: &[Cell<Motion>] = &motion::CELLS; $body }
            Params::Repeat($p) => { let $cells: &[Cell<Repeat>] = &repeat::CELLS; $body }
            Params::Reverb($p) => { let $cells: &[Cell<Reverb>] = &reverb::CELLS; $body }
            Params::Sidechain($p) => { let $cells: &[Cell<Sidechain>] = &sidechain::CELLS; $body }
            Params::Space($p) => { let $cells: &[Cell<Space>] = &space::CELLS; $body }
            Params::Suppressor($p) => { let $cells: &[Cell<Suppressor>] = &suppressor::CELLS; $body }
            Params::Time($p) => { let $cells: &[Cell<Time>] = &time::CELLS; $body }
            Params::Transient($p) => { let $cells: &[Cell<Transient>] = &transient::CELLS; $body }
            Params::Utility($p) => { let $cells: &[Cell<Utility>] = &utility::CELLS; $body }
            Params::VolumeLfo($p) => { let $cells: &[Cell<VolumeLfo>] = &volume_lfo::CELLS; $body }
        }
    };
}

pub(crate) trait Cells: Sized + 'static {
    const CELLS: &'static [Cell<Self>];
}

pub(super) fn param_of<P: Cells>(table: &'static [Param<P>], band: &str, name: &str) -> Option<&'static Param<P>> {
    if face::holds(P::CELLS, band, name) {
        return None;
    }
    param::find(table, band, name).or_else(|| face::switched(P::CELLS, band, name).map(|cell| &cell.param))
}

pub(super) fn params_of<P: Cells>(table: &[Param<P>]) -> Vec<Param<P>> {
    table.iter().copied().filter(|param| !face::holds(P::CELLS, param.band, param.name)).chain(face::switches(P::CELLS)).collect()
}

impl Faced for Slot {
    fn readout_rows(&self) -> i32 {
        match self.params {
            Params::Freak(_) => freak::READOUT_ROWS,
            _ => face::READOUT_ROWS,
        }
    }

    fn hover(&self, face: &mut Face, point: Option<(i32, i32)>) -> bool {
        let rows = self.readout_rows();
        each!(&self.params, |cells, _p| face.hover(point, rows, cells))
    }

    fn down(&mut self, face: &mut Face, x: i32, y: i32, bpm: f64) -> bool {
        let rows = self.readout_rows();
        each!(&mut self.params, |cells, p| face.down(x, y, rows, cells, p, bpm))
    }

    fn moved(&mut self, face: &mut Face, x: i32, y: i32) -> bool {
        let rows = self.readout_rows();
        each!(&mut self.params, |cells, p| face.moved(x, y, rows, cells, p))
    }

    fn wheel(&mut self, face: &mut Face, x: i32, y: i32, dy: i32) -> bool {
        let rows = self.readout_rows();
        each!(&mut self.params, |cells, p| face.wheel(x, y, dy, rows, cells, p))
    }

    fn up(&mut self, face: &mut Face, x: i32, y: i32) -> bool {
        let rows = self.readout_rows();
        each!(&mut self.params, |cells, p| face.up(x, y, rows, cells, p))
    }

    fn begin(&self, face: &mut Face, first: Option<char>) -> bool {
        each!(&self.params, |cells, p| face.begin(cells, p, first))
    }

    fn key(&mut self, face: &mut Face, key: Key, command: bool, bpm: f64) -> bool {
        each!(&mut self.params, |cells, p| face.key(key, command, cells, p, bpm))
    }

    fn nudge(&mut self, face: &mut Face, direction: i32, coarse: bool, bpm: f64) -> bool {
        each!(&mut self.params, |cells, p| face.nudge(cells, p, direction, coarse, bpm))
    }

    fn index_at(&self, x: i32, y: i32, view: View) -> Option<usize> {
        let rows = self.readout_rows();
        each!(&self.params, |cells, p| face::index_at(cells, p, rows, x, y, view))
    }

    fn setting_of(&self, cell: usize) -> Option<(&'static str, &'static str, f64)> {
        each!(&self.params, |cells, p| face::setting_at(cells, p, cell))
    }

    fn assign(&mut self, band: &str, param: &str, value: f64) -> Option<usize> {
        each!(&mut self.params, |cells, p| face::assign_named(cells, p, band, param, value))
    }

    fn restore(&mut self, cell: usize) -> Option<(&'static str, &'static str)> {
        each!(&mut self.params, |cells, p| face::restore_at(cells, p, cell))
    }

    fn armable(&self, cell: usize) -> Option<(&'static str, &'static str)> {
        each!(&self.params, |cells, _p| face::armable_cell(cells, cell))
    }

    fn armable_at(&self, x: i32, y: i32, view: View) -> Option<(&'static str, &'static str)> {
        let rows = self.readout_rows();
        each!(&self.params, |cells, p| face::armable_at(cells, p, rows, x, y, view))
    }

    fn automatable_of(&self, band: &str, param: &str) -> Option<(usize, &'static str, &'static str)> {
        each!(&self.params, |cells, _p| face::automatable_of(cells, band, param))
    }

    fn spelling(&self, band: &str, param: &str, travel: f64) -> Option<(String, &'static str)> {
        each!(&self.params, |cells, _p| face::spelling(cells, band, param, travel))
    }

    fn automated_rect(&self, band: &str, param: &str, view: View) -> Option<Rect> {
        let rows = self.readout_rows();
        each!(&self.params, |cells, _p| face::automated_rect(cells, rows, band, param, view))
    }
}

fn draw_filtered<P: Filtered>(face: &Face, list: &mut DrawList, rows: i32, cells: &[Cell<P>], patch: &P, open: usize) {
    let shown = face.shown_value(cells, patch).or_else(|| face::opener(cells, open).map(|index| (index, face::spelled(&cells[index], patch))));
    let site = patch.site(open);
    face.draw_pictured(list, rows, cells, patch, |list, inner| {
        if let Some((index, value)) = &shown {
            draw_filter(list, inner, &site, open, &cells[*index], value);
        }
    });
}

impl Slot {
    pub fn goes_big(&self) -> bool {
        matches!(self.params, Params::Eq(_) | Params::VolumeLfo(_))
    }

    pub fn views(&self) -> usize {
        match self.params {
            Params::Delay(_) => 3,
            Params::Freak(_) => 3,
            Params::Motion(_) => 2,
            Params::Reverb(_) => 2,
            _ => 1,
        }
    }

    fn band_of(&self, cell: usize) -> Option<usize> {
        match self.params {
            Params::Eq(_) if cell < EQ_BANDS * eq::BAND_CELLS => Some(cell / eq::BAND_CELLS),
            Params::Freak(_) => freak::CELLS.get(cell).and_then(|cell| cell.page).map(|page| page / self.views()),
            Params::Motion(_) => motion::CELLS.get(cell).and_then(|cell| cell.page).map(|page| page / self.views()),
            Params::Suppressor(_) => suppressor::CELLS.get(cell).and_then(|cell| cell.page),
            _ => None,
        }
    }

    fn page_of(&self, picture: &Picture, page: usize) -> usize {
        let views = self.views();
        match &self.params {
            Params::Freak(p) => picture.band.unwrap_or(0).min(p.count() - 1) * views + page % views,
            Params::Motion(p) => picture.band.unwrap_or(0).min(p.count() - 1) * views + page % views,
            Params::Suppressor(p) => picture.band.unwrap_or(0).min(p.count() - 1),
            Params::Eq(_) => page,
            _ => 0,
        }
    }

    pub fn press_picture(&mut self, picture: &mut Picture, face: &mut Face, x: i32, y: i32, inner: Rect, shift: bool, clicks: i32) -> bool {
        if let Some(open) = face.view().tray {
            return self.press_filter(picture, face, open, x, y, inner);
        }
        let pressed = picture.down(x, y, inner, self, shift, clicks);
        if pressed {
            face.turn(self.page_of(picture, face.page()));
        }
        pressed
    }

    fn press_filter(&self, picture: &mut Picture, face: &mut Face, open: usize, x: i32, y: i32, inner: Rect) -> bool {
        let Some(filtered) = self.params.filtered() else {
            return false;
        };
        if let Some(unit) = picture.down_filter(x, y, inner, &filtered.site(open))
            && let Some(index) = each!(&self.params, |cells, _p| face::opener(cells, unit))
        {
            face.open(index, unit);
        }
        true
    }

    pub fn right_picture(&mut self, picture: &mut Picture, face: &mut Face, x: i32, y: i32, inner: Rect) -> bool {
        if face.view().tray.is_some() {
            return false;
        }
        let pressed = picture.right(x, y, inner, self);
        if pressed {
            face.turn(self.page_of(picture, face.page()));
        }
        pressed
    }

    pub fn release_picture(&mut self, picture: &mut Picture, face: &mut Face) -> bool {
        let released = picture.up(Some(self));
        if released {
            face.turn(self.page_of(picture, face.page()));
        }
        released
    }

    pub fn follow_face(&self, face: &Face, picture: &mut Picture) {
        if let Some(band) = face.picked().and_then(|cell| self.band_of(cell)) {
            picture.band = Some(band);
        }
    }

    pub fn draw_big(&self, list: &mut DrawList, inner: Rect, picture: &Picture, readings: Readings) {
        let Readings { gauge, spectrum, .. } = readings;
        match &self.params {
            Params::Eq(p) => draw_curve(list, inner, p, picture.band, spectrum),
            Params::VolumeLfo(p) => draw_shape(list, inner, p, picture.sketch.node, gauge),
            _ => {}
        }
    }

    pub fn draw_plain(&self, face: &Face, list: &mut DrawList) {
        let rows = self.readout_rows();
        each!(&self.params, |cells, p| face.draw(list, rows, cells, p, |_, _| {}))
    }

    pub fn draw(&self, face: &Face, list: &mut DrawList, picture: &Picture, readings: Readings) {
        let Readings { gauge, deepest, signal, strips, spectrum, cuts } = readings;
        let rows = self.readout_rows();
        match (face.view().tray, &self.params) {
            (Some(open), Params::Delay(p)) => return draw_filtered(face, list, rows, &delay::CELLS, p, open),
            (Some(open), Params::Freak(p)) => return draw_filtered(face, list, rows, &freak::CELLS, p, open),
            (Some(open), Params::Motion(p)) => return draw_filtered(face, list, rows, &motion::CELLS, p, open),
            (Some(open), Params::Repeat(p)) => return draw_filtered(face, list, rows, &repeat::CELLS, p, open),
            (Some(open), Params::Reverb(p)) => return draw_filtered(face, list, rows, &reverb::CELLS, p, open),
            _ => {}
        }
        match &self.params {
            Params::Eq(p) => face.draw(list, rows, &eq::CELLS, p, |list, inner| draw_curve(list, inner, p, picture.band, spectrum)),
            Params::VolumeLfo(p) => face.draw(list, rows, &volume_lfo::CELLS, p, |list, inner| draw_shape(list, inner, p, picture.sketch.node, gauge)),
            Params::Compressor(p) => face.draw(list, rows, &compressor::CELLS, p, |list, inner| draw_reduction(list, inner, gauge)),
            Params::Suppressor(p) => face.draw(list, rows, &suppressor::CELLS, p, |list, inner| draw_suppress(list, inner, p, strips, spectrum, picture.marks(p), &cuts)),
            Params::Freak(p) => face.draw(list, rows, &freak::CELLS, p, |list, inner| draw_banded(list, inner, p, strips, spectrum, picture.marks(p), |_, _| {})),
            Params::Motion(p) => face.draw(list, rows, &motion::CELLS, p, |list, inner| draw_banded(list, inner, p, strips, spectrum, picture.marks(p), |_, _| {})),
            Params::Sidechain(p) => {
                let shown = face.shown_value(&sidechain::CELLS, p);
                face.draw_pictured(list, rows, &sidechain::CELLS, p, |list, inner| match &shown {
                    Some((index, value)) => draw_duck_tv(list, inner, signal, gauge, deepest, p.threshold_db as f32, &sidechain::CELLS[*index], value),
                    None => draw_duck(list, inner, signal, gauge, deepest, p.threshold_db as f32, VALUE_SCALE),
                });
            }
            Params::Space(p) => {
                let shown = face.shown_value(&space::CELLS, p);
                face.draw_pictured(list, rows, &space::CELLS, p, |list, inner| draw_space(list, inner, p, gauge, signal, spectrum, &cuts, shown.as_ref().map(|(index, value)| (&space::CELLS[*index], value.as_str()))));
            }
            Params::Gate(p) => {
                let shown = face.shown_value(&gate::CELLS, p);
                face.draw_pictured(list, rows, &gate::CELLS, p, |list, inner| match &shown {
                    Some((index, value)) => draw_duck_tv(list, inner, signal, gauge, gauge, p.threshold_db as f32, &gate::CELLS[*index], value),
                    None => draw_duck(list, inner, signal, gauge, gauge, p.threshold_db as f32, VALUE_SCALE),
                });
            }
            Params::Time(p) => {
                let shown = face.shown_value(&time::CELLS, p);
                let caption = shown.as_ref().map(|(index, value)| (&time::CELLS[*index], value.as_str()));
                face.draw_pictured(list, rows, &time::CELLS, p, |list, inner| draw_ramp(list, inner, caption, p, gauge, signal));
            }
            _ => self.draw_plain(face, list),
        }
    }
}

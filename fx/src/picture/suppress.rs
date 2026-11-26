use crate::params::suppressor::{BAND1, BAND2, BAND3, BAND4, SuppressorBand};
use crate::params::{FREAK_BANDS, Suppressor};
use sdk::draw::{DrawList, Rect, Tone};
use sdk::face::LABEL_SCALE;
use sdk::font;
use sdk::param::Param;
use sdk::spectrum::{self, CURVE, Spectrum};
use super::freak::{BandState, Banded, Collapsed, Crossovers, Marks, destroy_bands, draw_banded, split_bands};
use super::{LABEL_PAD, cut_at};

const FULL_DB: f32 = 24.0;
const DEPTH_LIFT_DB: f64 = 2.4;

impl Banded for Suppressor {
    const INS: [Param<Suppressor>; FREAK_BANDS] = [BAND1.depth, BAND2.depth, BAND3.depth, BAND4.depth];
    const OUTS: [Param<Suppressor>; FREAK_BANDS] = [BAND1.depth, BAND2.depth, BAND3.depth, BAND4.depth];
    const MIN_HZ: f64 = Suppressor::MIN_CROSSOVER_HZ;
    const MAX_HZ: f64 = Suppressor::MAX_CROSSOVER_HZ;
    const RATIO: f64 = Suppressor::CROSSOVER_RATIO;

    fn count(&self) -> usize {
        Suppressor::count(self)
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

    fn reducing(&self, _: usize) -> bool {
        false
    }

    fn output_db(&self, band: usize) -> f64 {
        self.bands[band].depth * DEPTH_LIFT_DB
    }

    fn spelled(&self, band: usize) -> String {
        format!("{:.1}", self.bands[band].depth)
    }

    fn split(&mut self, band: usize, hz: f64) -> bool {
        split_bands(&mut self.bands, &mut self.crossovers_hz, &mut self.band_count, band, hz, (Self::MIN_HZ, Self::MAX_HZ, Self::RATIO))
    }

    fn destroy(&mut self, gone: Collapsed) {
        destroy_bands(&mut self.bands, &mut self.band_count, gone, SuppressorBand::DEFAULT);
    }
}

pub(crate) fn draw_suppress(list: &mut DrawList, inner: Rect, p: &Suppressor, strips: [[f32; 2]; FREAK_BANDS], spectrum: Option<&Spectrum>, marks: Marks, cuts: &[f32; CURVE]) {
    draw_banded(list, inner, p, strips, spectrum, marks, |list, area| {
        if area.w <= 0 || area.h <= 0 {
            return;
        }
        let middle = area.y + area.h / 2;
        let full = area.h / 2 - LABEL_PAD - font::GLYPH_H * LABEL_SCALE;
        for column in 0..area.w {
            let cut = cut_at(cuts, spectrum::hz(area.w, column));
            let h = ((cut / FULL_DB).clamp(0.0, 1.0) * full as f32).round() as i32;
            if h > 0 {
                list.fill(Rect::new(area.x + column, middle + 1, 1, h), Tone::Ink75);
                list.fill(Rect::new(area.x + column, middle + h, 1, 1), Tone::Ink100);
            }
        }
    });
}

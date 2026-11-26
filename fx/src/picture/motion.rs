use crate::params::motion::{BAND1, BAND2, BAND3, BAND4, MotionBand};
use crate::params::{FREAK_BANDS, MOTION_BANDS, Motion};
use sdk::param::Param;
use super::freak::{BandState, Banded, Collapsed, Crossovers};

impl Banded for Motion {
    const INS: [Param<Motion>; FREAK_BANDS] = [BAND1.input, BAND2.input, BAND3.input, BAND4.input];
    const OUTS: [Param<Motion>; FREAK_BANDS] = [BAND1.output, BAND2.output, BAND3.output, BAND4.output];
    const MIN_HZ: f64 = Motion::MIN_CROSSOVER_HZ;
    const MAX_HZ: f64 = Motion::MAX_CROSSOVER_HZ;
    const RATIO: f64 = Motion::CROSSOVER_RATIO;

    fn count(&self) -> usize {
        Motion::count(self)
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
        self.bands[band].output_db
    }

    fn split(&mut self, band: usize, hz: f64) -> bool {
        let count = self.count();
        if count >= MOTION_BANDS {
            return false;
        }
        let ratio = Motion::CROSSOVER_RATIO;
        let low = if band == 0 { Motion::MIN_CROSSOVER_HZ } else { self.crossovers_hz[band - 1] * ratio };
        let high = if band + 1 == count { Motion::MAX_CROSSOVER_HZ } else { self.crossovers_hz[band] / ratio };
        if low > high {
            return false;
        }
        self.bands.copy_within(band..count, band + 1);
        self.crossovers_hz.copy_within(band..count - 1, band + 1);
        self.crossovers_hz[band] = hz.clamp(low, high);
        self.band_count = count + 1;
        true
    }

    fn destroy(&mut self, gone: Collapsed) {
        let count = self.count();
        let survivors = self.bands.iter().zip(gone).take(count).filter(|(_, gone)| !gone).map(|(settings, _)| *settings);
        let mut bands = [MotionBand::DEFAULT; MOTION_BANDS];
        for (slot, settings) in bands.iter_mut().zip(survivors) {
            *slot = settings;
        }
        self.bands = bands;
        self.band_count = count - gone.iter().filter(|gone| **gone).count();
    }
}

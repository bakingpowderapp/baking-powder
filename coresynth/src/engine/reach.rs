use crate::params::Modulation;

#[derive(Clone, Copy, Debug)]
pub(super) struct Reach {
    pub(super) cutoff: f64,
    pub(super) pan: f64,
    pub(super) width: f64,
}

impl Reach {
    pub(super) fn of(modulation: &Modulation) -> Reach {
        Reach { cutoff: modulation.cutoff, pan: modulation.pan, width: modulation.pulse_width }
    }

    pub(super) fn at(&self, modulation: f64) -> Reach {
        Reach { cutoff: self.cutoff * modulation, pan: self.pan * modulation, width: self.width * modulation }
    }
}

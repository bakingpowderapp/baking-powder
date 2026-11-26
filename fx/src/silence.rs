const FAINTEST: f32 = 1e-6;
const LASTING_SECONDS: f64 = 0.05;

#[derive(Clone, Copy, Default)]
pub(crate) struct Silence {
    frames: usize,
}

impl Silence {
    pub(crate) fn trailing(l: &[f32], r: &[f32]) -> usize {
        l.iter().zip(r).rev().take_while(|(l, r)| l.abs() <= FAINTEST && r.abs() <= FAINTEST).count()
    }

    pub(crate) fn passed(&mut self, trailing: usize, frames: usize) {
        self.frames = if trailing < frames { trailing } else { self.frames.saturating_add(frames) };
    }

    pub(crate) fn lasting(&self, sample_rate: f64) -> bool {
        self.frames as f64 >= LASTING_SECONDS * sample_rate
    }
}

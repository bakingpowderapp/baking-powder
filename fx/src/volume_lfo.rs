use sdk::dsp;
use crate::{Clock, Unit, params};

const GAIN_SMOOTH_SECONDS: f64 = 0.003;

pub struct VolumeLfo {
    sample_rate: f64,
    cycle: Option<f64>,
    gain: dsp::Smooth,
    live: f32,
}

impl Unit for VolumeLfo {
    type Params = params::VolumeLfo;

    fn new(sample_rate: f64, _: usize) -> VolumeLfo {
        VolumeLfo { sample_rate, cycle: None, gain: dsp::Smooth::new(1.0, GAIN_SMOOTH_SECONDS, sample_rate), live: 0.0 }
    }

    fn restart(&mut self) {
        self.cycle = None;
    }

    fn gauge(&self) -> f32 {
        self.live
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], params: &params::VolumeLfo, wet: f32, clock: &Clock) {
        dsp::sanitize(l, r);
        let rate = clock.rate(params.sync, params.division, params.rate_hz);
        let wet = f64::from(wet);
        let target = |cycle: f64| 1.0 - wet + wet * params.value_at(cycle + params.phase);
        let n = l.len().min(r.len());
        let cycle = match clock.beat {
            Some(beat) => {
                let per_beat = rate * 60.0 / clock.bpm.max(1.0);
                let at = |frame: usize| {
                    let cycles = beat.at(frame) * per_beat;
                    cycles - cycles.floor()
                };
                if self.cycle.is_none() {
                    self.gain.snap(target(at(0)));
                }
                for i in 0..n {
                    let gain = self.gain.tick(target(at(i))) as f32;
                    l[i] *= gain;
                    r[i] *= gain;
                }
                at(n)
            }
            None => {
                let mut cycle = self.cycle.unwrap_or(0.0);
                if self.cycle.is_none() {
                    self.gain.snap(target(cycle));
                }
                let step = rate / self.sample_rate;
                for i in 0..n {
                    let gain = self.gain.tick(target(cycle)) as f32;
                    l[i] *= gain;
                    r[i] *= gain;
                    cycle = dsp::wrap(cycle + step);
                }
                cycle
            }
        };
        self.cycle = Some(cycle);
        let read = cycle + params.phase;
        self.live = (read - read.floor()) as f32;
    }
}

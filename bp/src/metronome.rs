use crate::project::BEATS_PER_BAR;

const DOWNBEAT_HZ: f64 = 1000.0;
const BEAT_HZ: f64 = 800.0;
const AMPLITUDE: f32 = 0.35;
const DECAY_SECONDS: f64 = 0.030;
const DECAY_RATIO: f64 = 1000.0;
const FLOOR: f32 = 1e-4;

pub struct Metronome {
    sample_rate: f64,
    decay: f32,
    envelope: f32,
    phase: f64,
    step: f64,
}

impl Metronome {
    pub fn new(sample_rate: f64) -> Metronome {
        let decay = (-DECAY_RATIO.ln() / (DECAY_SECONDS * sample_rate)).exp() as f32;
        Metronome { sample_rate, decay, envelope: 0.0, phase: 0.0, step: 0.0 }
    }

    pub fn render(&mut self, l: &mut [f32], r: &mut [f32], beat: Option<f64>, frames_per_beat: f64) {
        let n = l.len().min(r.len());
        let mut at = 0;
        if let Some(start) = beat {
            let half_frame = 0.5 / frames_per_beat;
            let mut next = (start - half_frame).ceil();
            loop {
                let frame = ((next - start) * frames_per_beat).round().max(0.0) as usize;
                if frame >= n {
                    break;
                }
                self.ring(&mut l[at..frame], &mut r[at..frame]);
                self.strike(next.rem_euclid(f64::from(BEATS_PER_BAR)) == 0.0);
                at = frame;
                next += 1.0;
            }
        }
        self.ring(&mut l[at..n], &mut r[at..n]);
    }

    fn strike(&mut self, downbeat: bool) {
        self.envelope = 1.0;
        self.phase = 0.0;
        self.step = if downbeat { DOWNBEAT_HZ } else { BEAT_HZ } / self.sample_rate;
    }

    fn ring(&mut self, l: &mut [f32], r: &mut [f32]) {
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            if self.envelope < FLOOR {
                self.envelope = 0.0;
                return;
            }
            let click = (std::f64::consts::TAU * self.phase).sin() as f32 * AMPLITUDE * self.envelope;
            self.phase = (self.phase + self.step).fract();
            self.envelope *= self.decay;
            *a += click;
            *b += click;
        }
    }
}

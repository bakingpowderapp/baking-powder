use sdk::dsp::filter::{State, Tuned};
use sdk::dsp::{Ring, db_to_gain, sample_hold, sanitize};
use crate::params::{self, Grid, RepeatMix, Vary};
use crate::{Clock, Unit};

const RING_SECONDS: f64 = 16.0;
const FADE_SECONDS: f64 = 0.002;
const WINDOW_SECONDS: f64 = 0.004;
const QUIET_SECONDS: f64 = 0.05;
const SPREAD: f64 = 6.0;
const CHANCE_SEED: u64 = 0xBEA7_5EED_C4A2_CE01;
const GRID_SEED: u64 = 0xBEA7_5EED_62D1_0A02;
const VARY_SEED: u64 = 0xBEA7_5EED_A7B4_1E03;
const SLOT_STRIDE: u64 = 0x9E37_79B9_7F4A_7C15;
const FILTERS: usize = params::Repeat::FILTERS;

struct Run {
    slot: i64,
    began: i64,
    end: i64,
    length: f64,
    pending: Option<f64>,
    position: f64,
    pass: u32,
    vary_slot: i64,
    capturing: bool,
}

impl Run {
    fn window(&self, frame: i64, window: f64) -> f64 {
        let opened = (frame - self.began) as f64 / window;
        let closing = (self.end - frame) as f64 / window;
        opened.min(closing).clamp(0.0, 1.0)
    }

    fn close(&mut self, by: i64) {
        self.end = self.end.min(by);
    }
}

struct Voice {
    sample: (f32, f32),
    first: bool,
    window: f32,
}

pub struct Repeat {
    sample_rate: f64,
    rings: [Ring; 2],
    capacity: f64,
    frame: i64,
    run: Option<Run>,
    fading: Option<Run>,
    filters: [Tuned; FILTERS],
    cut: [[State; FILTERS]; 2],
    quiet: usize,
}

fn drawn(seed: u64, slot: i64, turn: u32) -> f64 {
    0.5 * (sample_hold(i64::from(turn), seed ^ (slot as u64).wrapping_mul(SLOT_STRIDE)) + 1.0)
}

fn grid(p: &params::Repeat, frames_per_beat: f64, draw: f64) -> f64 {
    let base = p.grid.index() as isize;
    let last = Grid::ALL.len() as isize - 1;
    let steps = ((draw * 2.0 - 1.0) * p.variation * SPREAD).round() as isize;
    let index = (base + steps).clamp(0, last);
    Grid::ALL[index as usize].beats() * frames_per_beat
}

fn vary_slot(vary: Vary, beat: f64) -> i64 {
    match vary {
        Vary::Quarter => beat.floor() as i64,
        Vary::Eighth => (beat * 2.0).floor() as i64,
        Vary::Sixteenth => (beat * 4.0).floor() as i64,
        Vary::Trigger | Vary::Auto => 0,
    }
}

impl Repeat {
    fn window_frames(&self) -> i64 {
        (WINDOW_SECONDS * self.sample_rate) as i64
    }

    fn start(&mut self, p: &params::Repeat, frames_per_beat: f64, slot: i64, end: i64, beat: f64) {
        let length = grid(p, frames_per_beat, drawn(GRID_SEED, slot, 0));
        let by = self.frame + self.window_frames();
        if let Some(mut old) = self.run.take() {
            old.close(by);
            self.fading = Some(old);
        }
        self.run = Some(Run { slot, began: self.frame, end, length, pending: None, position: 0.0, pass: 0, vary_slot: vary_slot(p.vary, beat), capturing: true });
    }

    fn voice(&self, run: &mut Run, p: &params::Repeat, frames_per_beat: f64, beat: f64, fade: f64, window: f64, volume: f32) -> Option<Voice> {
        let age = (self.frame - run.began) as f64;
        if self.frame >= run.end || age + 2.0 >= self.capacity {
            return None;
        }
        let vary = vary_slot(p.vary, beat);
        if vary != run.vary_slot {
            run.vary_slot = vary;
            run.pending = Some(grid(p, frames_per_beat, drawn(VARY_SEED, vary, 0)));
        }
        let first = run.capturing;
        let edge = if first { run.length - run.position } else { run.position.min(run.length - run.position) };
        let window = run.window(self.frame, window) as f32;
        let gain = (edge / fade).clamp(0.0, 1.0) as f32 * window * (1.0 - p.decay as f32).powi(run.pass as i32) * volume;
        let delay = (age - run.position + 1.0).max(1.0);
        let sample = (self.rings[0].linear(delay) * gain, self.rings[1].linear(delay) * gain);
        run.position += 1.0;
        if run.position >= run.length {
            run.position -= run.length;
            run.pass += 1;
            run.capturing = false;
            if p.vary == Vary::Auto {
                run.pending = Some(grid(p, frames_per_beat, drawn(GRID_SEED, run.slot, run.pass)));
            }
            if let Some(length) = run.pending.take() {
                run.length = length;
                run.position %= run.length;
            }
        }
        Some(Voice { sample, first, window })
    }

    fn voices(&mut self, p: &params::Repeat, frames_per_beat: f64, beat: f64, fade: f64, window: f64, volume: f32) -> ((f32, f32), f32) {
        let (mut wet, mut open) = ((0.0, 0.0), 0.0);
        for slot in [true, false] {
            let Some(mut run) = (if slot { self.run.take() } else { self.fading.take() }) else {
                continue;
            };
            if let Some(voice) = self.voice(&mut run, p, frames_per_beat, beat, fade, window, volume) {
                if p.mix != RepeatMix::Mix || !voice.first {
                    wet = (wet.0 + voice.sample.0, wet.1 + voice.sample.1);
                }
                open += voice.window;
                if slot {
                    self.run = Some(run);
                } else {
                    self.fading = Some(run);
                }
            }
        }
        (wet, open.min(1.0))
    }
}

impl Unit for Repeat {
    type Params = params::Repeat;

    fn new(sample_rate: f64, _: usize) -> Repeat {
        let frames = (RING_SECONDS * sample_rate) as usize;
        Repeat { sample_rate, rings: [Ring::new(frames), Ring::new(frames)], capacity: frames as f64, frame: 0, run: None, fading: None, filters: params::Repeat::DEFAULT.filters().map(|filter| Tuned::of(&filter, sample_rate)), cut: [[State::default(); FILTERS]; 2], quiet: 0 }
    }

    fn restart(&mut self) {
        self.run = None;
        self.fading = None;
        self.frame = 0;
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], p: &params::Repeat, wet: f32, clock: &Clock) {
        sanitize(l, r);
        let frames_per_beat = clock.beat.map_or(clock.sample_rate * 60.0 / clock.bpm.max(1.0), |beat| beat.frames_per_beat);
        let interval = p.interval.beats();
        let offset = p.offset as f64 * 0.25;
        let gate = (p.gate as f64 * 0.25 * frames_per_beat) as i64;
        let fade = FADE_SECONDS * self.sample_rate;
        let window = WINDOW_SECONDS * self.sample_rate;
        let volume = db_to_gain(p.volume_db) as f32;
        for (tuned, filter) in self.filters.iter_mut().zip(p.filters()) {
            tuned.tune(&filter, self.sample_rate);
        }
        let filters = self.filters;
        let cut = |states: &mut [State; FILTERS], x: f32| states.iter_mut().zip(&filters).fold(x, |y, (state, tuned)| state.tick(y, tuned.path, &tuned.coeffs));
        let slot_at = |beat: f64| ((beat - offset) / interval).floor() as i64;
        let settled = (QUIET_SECONDS * self.sample_rate) as usize;
        for i in 0..l.len().min(r.len()) {
            let (before, beat) = match clock.beat {
                Some(beat) => (beat.anchor + (beat.since + i as i64 - 1) as f64 / beat.frames_per_beat, beat.at(i)),
                None => ((self.frame - 1) as f64 / frames_per_beat, self.frame as f64 / frames_per_beat),
            };
            self.rings[0].push(l[i]);
            self.rings[1].push(r[i]);
            let slot = slot_at(beat);
            if beat >= 0.0 && slot > slot_at(before) && drawn(CHANCE_SEED, slot, 0) < p.chance {
                self.start(p, frames_per_beat, slot, self.frame + gate, beat);
            }
            let dry = (l[i], r[i]);
            let (played, open) = self.voices(p, frames_per_beat, beat, fade, window, volume);
            let repeats = (cut(&mut self.cut[0], played.0), cut(&mut self.cut[1], played.1));
            let faint = played == (0.0, 0.0) && repeats.0.abs() < f32::MIN_POSITIVE && repeats.1.abs() < f32::MIN_POSITIVE;
            self.quiet = if faint { self.quiet + 1 } else { 0 };
            if self.quiet == settled {
                self.cut = [[State::default(); FILTERS]; 2];
            }
            let out = match p.mix {
                RepeatMix::Mix => (dry.0 + repeats.0, dry.1 + repeats.1),
                RepeatMix::Insert => (dry.0 * (1.0 - open) + repeats.0, dry.1 * (1.0 - open) + repeats.1),
                RepeatMix::Gate => repeats,
            };
            l[i] = dry.0 + (out.0 - dry.0) * wet;
            r[i] = dry.1 + (out.1 - dry.1) * wet;
            self.frame += 1;
        }
    }
}

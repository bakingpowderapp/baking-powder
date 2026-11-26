use super::chorus::Chorus;
use super::flanger::Flanger;
use super::phaser::Phaser;
use super::split::{Split, Voicing};
use sdk::dsp::crossover;
use sdk::dsp::{Ring, db_to_gain, sanitize, settle, smoothing};
use crate::params::motion::MotionBand;
use crate::params::{self, MOTION_BANDS, MotionMode};
use crate::silence::Silence;
use crate::{Beat, Clock, Unit};

const BANDS: usize = MOTION_BANDS;
const SMOOTH_SECONDS: f64 = 0.030;
const PLAIN_FADE_SECONDS: f64 = 0.010;
const LEVEL_FLOOR_DB: f32 = -96.0;

fn level_of(peak: f32) -> f32 {
    (20.0 * peak.log10()).clamp(LEVEL_FLOOR_DB, 0.0)
}

#[derive(Clone, Copy)]
struct BandLevels {
    input: f32,
    output: f32,
    heard: f32,
}

impl BandLevels {
    fn target(band: &MotionBand, soloing: bool) -> BandLevels {
        let audible = if soloing { band.solo } else { !band.mute };
        let trim = |db: f64| if band.on { db_to_gain(db) as f32 } else { 1.0 };
        BandLevels { input: trim(band.input_db), output: trim(band.output_db), heard: if audible { 1.0 } else { 0.0 } }
    }

    fn values(self) -> [f32; 3] {
        [self.input, self.output, self.heard]
    }

    fn toward(&mut self, target: &BandLevels, a: f32) {
        self.input = settle(self.input, target.input, a);
        self.output = settle(self.output, target.output, a);
        self.heard = settle(self.heard, target.heard, a);
    }
}

#[derive(Clone, Copy)]
struct Levels {
    output: f32,
    bands: [BandLevels; BANDS],
}

impl Levels {
    fn target(params: &params::Motion) -> Levels {
        let soloing = params.bands[..params.count()].iter().any(|band| band.solo);
        Levels { output: db_to_gain(params.output_db) as f32, bands: params.bands.each_ref().map(|band| BandLevels::target(band, soloing)) }
    }

    fn toward(&mut self, target: &Levels, a: f32) {
        self.output = settle(self.output, target.output, a);
        for (band, t) in self.bands.iter_mut().zip(&target.bands) {
            band.toward(t, a);
        }
    }

    fn still(&self, target: &Levels, a: f32) -> bool {
        let mut next = *self;
        next.toward(target, a);
        next.bits().eq(self.bits())
    }

    fn bits(&self) -> impl Iterator<Item = u32> {
        [self.output].into_iter().chain(self.bands.into_iter().flat_map(BandLevels::values)).map(f32::to_bits)
    }
}

struct Stages {
    chorus: Chorus,
    flanger: Flanger,
    phaser: Phaser,
}

impl Stages {
    fn new(sample_rate: f64, max_frames: usize) -> Stages {
        Stages { chorus: Chorus::new(sample_rate, max_frames), flanger: Flanger::new(sample_rate, max_frames), phaser: Phaser::new(sample_rate, max_frames) }
    }

    fn clear(&mut self, mode: MotionMode) {
        match mode {
            MotionMode::None => {}
            MotionMode::Chorus => self.chorus.clear(),
            MotionMode::Flanger => self.flanger.clear(),
            MotionMode::Phaser => self.phaser.clear(),
        }
    }

    fn restart(&mut self) {
        self.chorus.restart();
        self.flanger.restart();
        self.phaser.restart();
    }

    fn run(&mut self, mode: MotionMode, l: &mut [f32], r: &mut [f32], band: &MotionBand, wet: f32, clock: &Clock) {
        match mode {
            MotionMode::None => {}
            MotionMode::Chorus => self.chorus.process(l, r, &band.chorus, wet, clock),
            MotionMode::Flanger => self.flanger.process(l, r, &band.flanger, wet, clock),
            MotionMode::Phaser => self.phaser.process(l, r, &band.phaser, wet, clock),
        }
    }
}

pub struct Motion {
    sample_rate: f64,
    room: usize,
    smooth: f32,
    level: Option<Levels>,
    level_settled_for: Option<Levels>,
    split: Split,
    stages: [Stages; BANDS],
    running: [Option<MotionMode>; BANDS],
    plain_fade: Option<usize>,
    line: [Ring; 2],
    strips: [[f32; 2]; BANDS],
    silence: Silence,
}

fn voicing(params: &params::Motion) -> Voicing {
    Voicing::new(params.crossovers_hz, params.slope, params.count(), params::Motion::CROSSOVER_RATIO)
}

impl Motion {
    fn block(&mut self, l: &mut [f32], r: &mut [f32], params: &params::Motion, wet: f32, clock: &Clock) {
        let n = l.len();
        let sr = self.sample_rate;
        let count = params.count();
        let delay = params.latency(sr) + 1;
        let engaged = params.engaged();
        let smooth = self.smooth;
        let target = Levels::target(params);
        let mut level = *self.level.get_or_insert(target);
        level.bands[count..].copy_from_slice(&target.bands[count..]);
        let level_moves = !self.level_settled_for.is_some_and(|settled| settled.bits().eq(target.bits())) && !level.still(&target, smooth);
        let start = level;
        self.split.run(l, r, voicing(params));
        let mut peaks = [0.0f32; BANDS];
        for (b, peak) in peaks.iter_mut().enumerate().take(count) {
            let band = &params.bands[b];
            let (from, to) = (start.bands[b], target.bands[b]);
            let [bl, br] = &mut self.split.bands_mut()[b];
            let (bl, br) = (&mut bl[..n], &mut br[..n]);
            let mut input = from.input;
            for (xl, xr) in bl.iter_mut().zip(br.iter_mut()) {
                if level_moves {
                    input = settle(input, to.input, smooth);
                }
                *xl *= input;
                *xr *= input;
            }
            let mode = (band.on && band.modulating()).then_some(band.mode);
            if let Some(mode) = mode {
                if self.running[b] != Some(mode) {
                    self.stages[b].clear(mode);
                }
                self.stages[b].run(mode, bl, br, band, wet * band.wet as f32, clock);
            }
            self.running[b] = mode;
            let (mut output, mut heard) = (from.output, from.heard);
            for (xl, xr) in bl.iter_mut().zip(br.iter_mut()) {
                if level_moves {
                    output = settle(output, to.output, smooth);
                    heard = settle(heard, to.heard, smooth);
                }
                let gain = output * heard;
                *xl *= gain;
                *xr *= gain;
                *peak = peak.max(xl.abs()).max(xr.abs());
            }
            level.bands[b] = BandLevels { input, output, heard };
        }
        for running in &mut self.running[count..] {
            *running = None;
        }
        let plain_len = ((PLAIN_FADE_SECONDS * sr) as usize).max(1);
        let mut plain_fade = *self.plain_fade.get_or_insert(if engaged { plain_len } else { 0 });
        let mut trim = start.output;
        for i in 0..n {
            if level_moves {
                trim = settle(trim, target.output, smooth);
            }
            let x = [l[i], r[i]];
            let mut plain = [0.0f32; 2];
            for (line, (slot, x)) in self.line.iter_mut().zip(plain.iter_mut().zip(x)) {
                line.push(x);
                *slot = line.tap(delay);
            }
            let bands = self.split.bands();
            let mut sum = [bands[0][0][i], bands[0][1][i]];
            for signal in &bands[1..count] {
                sum[0] += signal[0][i];
                sum[1] += signal[1][i];
            }
            let out = [sum[0] * trim, sum[1] * trim];
            plain_fade = if engaged { (plain_fade + 1).min(plain_len) } else { plain_fade.saturating_sub(1) };
            let out = if plain_fade >= plain_len {
                out
            } else if plain_fade == 0 {
                plain
            } else {
                let m = plain_fade as f32 / plain_len as f32;
                [plain[0] + m * (out[0] - plain[0]), plain[1] + m * (out[1] - plain[1])]
            };
            l[i] = out[0];
            r[i] = out[1];
        }
        level.output = trim;
        self.level = Some(level);
        self.level_settled_for = (!level_moves).then_some(target);
        self.plain_fade = Some(plain_fade);
        self.strips = peaks.map(|peak| [0.0, level_of(peak)]);
    }
}

impl Unit for Motion {
    type Params = params::Motion;

    fn new(sample_rate: f64, max_frames: usize) -> Motion {
        let room = max_frames.max(1);
        Motion {
            sample_rate,
            room,
            smooth: smoothing(SMOOTH_SECONDS, sample_rate) as f32,
            level: None,
            level_settled_for: None,
            split: Split::new(sample_rate, room),
            stages: std::array::from_fn(|_| Stages::new(sample_rate, max_frames)),
            running: [None; BANDS],
            plain_fade: None,
            line: std::array::from_fn(|_| Ring::new(crossover::half(sample_rate) + 1)),
            strips: [[0.0; 2]; BANDS],
            silence: Silence::default(),
        }
    }

    fn restart(&mut self) {
        for stages in &mut self.stages {
            stages.restart();
        }
        if self.silence.lasting(self.sample_rate) {
            self.split.renew();
            for line in &mut self.line {
                line.clear();
            }
            (self.level, self.level_settled_for, self.plain_fade) = (None, None, None);
            self.running = [None; BANDS];
        }
    }

    fn strips(&self) -> [[f32; 2]; params::FREAK_BANDS] {
        self.strips
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], params: &params::Motion, wet: f32, clock: &Clock) {
        sanitize(l, r);
        let quiet = Silence::trailing(l, r);
        let n = l.len().min(r.len());
        for start in (0..n.max(1)).step_by(self.room) {
            let end = (start + self.room).min(n);
            let clock = Clock { beat: clock.beat.map(|beat| Beat { since: beat.since + start as i64, ..beat }), ..*clock };
            self.block(&mut l[start..end], &mut r[start..end], params, wet, &clock);
        }
        self.silence.passed(quiet.min(Silence::trailing(l, r)), n);
    }
}

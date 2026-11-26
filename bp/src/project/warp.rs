use super::BEATS_PER_BAR;
use crate::wav::Wave;
use std::sync::Arc;

const HOP_SECONDS: f64 = 0.001;
const SMOOTH_HOPS: usize = 1;
const RISE_HOPS: usize = 3;
const RISE_DB: f64 = 10.0;
const CREST_HOPS: usize = 5;
const APART_HOPS: usize = 50;
const PEAK_HOPS: usize = 10;
const GUARD_HOPS: usize = 20;
const LOUDER_DB: f64 = 3.0;
const BACK_HOPS: usize = 1;
const FLOOR: f64 = 1e-10;
const FULL_SCALE: f64 = 32_768.0;
const PRE_ROLL_SECONDS: f64 = 0.010;
const FADE_SECONDS: f64 = 0.0015;
const CROSSING_SECONDS: f64 = 0.010;
const DECAY_SECONDS: f64 = 0.1;
const LEAST_DECAY_FRAMES: f64 = 1.0;
const BARS: [f64; 5] = [1.0, 2.0, 4.0, 8.0, 16.0];
const TEMPO_LEAST: u32 = 60;
const TEMPO_MOST: u32 = 200;
const NEAREST_OCTAVES: f64 = 0.5;
pub const ENVELOPE_MAX: f32 = 100.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Loop {
    #[default]
    Rest,
    Forward,
    BackAndForth,
}

impl Loop {
    pub const ALL: [Loop; 3] = [Loop::Rest, Loop::Forward, Loop::BackAndForth];
    pub const WORDS: [&'static str; 3] = ["REST", "FWD", "BAF"];

    pub fn name(self) -> &'static str {
        match self {
            Loop::Rest => "rest",
            Loop::Forward => "forward",
            Loop::BackAndForth => "backAndForth",
        }
    }

    pub fn parse(name: &str) -> Loop {
        Loop::ALL.into_iter().find(|mode| mode.name() == name).unwrap_or_default()
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Segment {
    pub marker: f64,
    mid: f64,
    turn: f64,
}

pub struct Beats {
    rate: f64,
    frames: f64,
    segments: Vec<Segment>,
}

pub fn tempo_of(name: &str, seconds: f64, song_bpm: f64) -> Option<f64> {
    let token_tempo = |token: &str| token.strip_suffix("bpm").or_else(|| token.strip_suffix("BPM")).unwrap_or(token).parse::<u32>().ok().filter(|tempo| (TEMPO_LEAST..=TEMPO_MOST).contains(tempo));
    if let Some(tempo) = name.split(|c: char| !c.is_ascii_alphanumeric()).find_map(token_tempo) {
        return Some(f64::from(tempo));
    }
    if seconds <= 0.0 || song_bpm <= 0.0 {
        return None;
    }
    let distance = |tempo: f64| (tempo / song_bpm).log2().abs();
    let guess = BARS.iter().map(|bars| bars * f64::from(BEATS_PER_BAR) * 60.0 / seconds).min_by(|a, b| distance(*a).total_cmp(&distance(*b)))?;
    (distance(guess) <= NEAREST_OCTAVES).then_some(guess)
}

pub fn decay_frames(envelope: f32, rate: f64) -> f64 {
    let held = f64::from(envelope.clamp(0.0, ENVELOPE_MAX));
    if held >= f64::from(ENVELOPE_MAX) { f64::INFINITY } else { (DECAY_SECONDS * rate * held / (f64::from(ENVELOPE_MAX) - held)).max(LEAST_DECAY_FRAMES) }
}

fn hop_powers(wave: &Wave, hop: usize) -> (Vec<f64>, Vec<f64>) {
    let hops = wave.frames() / hop;
    let (mut full, mut high) = (Vec::with_capacity(hops), Vec::with_capacity(hops));
    let mut last = 0.0;
    for h in 0..hops {
        let (mut sum, mut rise) = (0.0, 0.0);
        for frame in h * hop..(h + 1) * hop {
            let x = wave.mono(frame) / FULL_SCALE;
            sum += x * x;
            rise += (x - last) * (x - last);
            last = x;
        }
        full.push(sum / hop as f64);
        high.push(rise / hop as f64);
    }
    (full, high)
}

fn decibels(power: &[f64]) -> Vec<f64> {
    let raw: Vec<f64> = power.iter().map(|p| 10.0 * (p + FLOOR).log10()).collect();
    (0..raw.len())
        .map(|i| {
            let (lo, hi) = (i.saturating_sub(SMOOTH_HOPS), (i + SMOOTH_HOPS + 1).min(raw.len()));
            raw[lo..hi].iter().sum::<f64>() / (hi - lo) as f64
        })
        .collect()
}

pub fn markers(wave: &Wave) -> Vec<f64> {
    let hop = (wave.rate * HOP_SECONDS).round().max(1.0) as usize;
    let (full, high) = hop_powers(wave, hop);
    let (full, high) = (decibels(&full), decibels(&high));
    let hops = full.len();
    let flux: Vec<f64> = (0..hops).map(|i| if i < RISE_HOPS { 0.0 } else { (full[i] - full[i - RISE_HOPS]).max(0.0) + (high[i] - high[i - RISE_HOPS]).max(0.0) }).collect();
    let crest = |i: usize| {
        let (lo, hi) = (i.saturating_sub(CREST_HOPS), (i + CREST_HOPS + 1).min(hops));
        flux[i] >= RISE_DB && flux[lo..i].iter().all(|&f| f < flux[i]) && flux[i..hi].iter().all(|&f| f <= flux[i])
    };
    let reached = |i: usize| full[i..(i + PEAK_HOPS).min(hops)].iter().copied().fold(f64::MIN, f64::max);
    let mut kept: Vec<(usize, f64)> = Vec::new();
    for i in (GUARD_HOPS..hops.saturating_sub(GUARD_HOPS)).filter(|&i| crest(i)) {
        let level = reached(i);
        match kept.last_mut() {
            Some(last) if i - last.0 < APART_HOPS => {
                if level > last.1 + LOUDER_DB {
                    *last = (i, level);
                }
            }
            _ => kept.push((i, level)),
        }
    }
    std::iter::once(0.0).chain(kept.into_iter().map(|(i, _)| ((i - BACK_HOPS) * hop) as f64)).collect()
}

fn crossing_near(wave: &Wave, target: f64, reach: usize, lo: usize, hi: usize) -> f64 {
    if hi <= lo + 1 {
        return target;
    }
    let centre = (target.round().max(0.0) as usize).clamp(lo, hi - 1);
    let crosses = |at: usize| at > lo && at < hi && (wave.mono(at - 1) <= 0.0) != (wave.mono(at) <= 0.0);
    for offset in 0..=reach {
        for at in [centre + offset, centre.saturating_sub(offset)] {
            if crosses(at) {
                return at as f64;
            }
        }
    }
    target
}

impl Beats {
    pub fn of(wave: &Wave) -> Beats {
        let rate = wave.rate;
        let frames = wave.frames() as f64;
        let markers = markers(wave);
        let reach = (rate * CROSSING_SECONDS).round() as usize;
        let pre = rate * PRE_ROLL_SECONDS;
        let segments = markers
            .iter()
            .enumerate()
            .map(|(k, &marker)| {
                let next = markers.get(k + 1).copied().unwrap_or(frames);
                let mid = crossing_near(wave, (marker + next) * 0.5, reach, marker as usize, next as usize);
                let turn = crossing_near(wave, (next - pre).max(mid), reach, mid as usize + 1, next as usize);
                Segment { marker, mid, turn }
            })
            .collect();
        Beats { rate, frames, segments }
    }

    fn cursor_at(&self, d: f64, ratio: f64) -> usize {
        let pre = self.rate * PRE_ROLL_SECONDS;
        self.segments.partition_point(|segment| segment.marker * ratio - pre <= d).saturating_sub(1)
    }

    fn source_at(&self, d: f64, ratio: f64, cursor: usize) -> f64 {
        let Some(segment) = self.segments.get(cursor) else {
            return (d / ratio).clamp(0.0, self.frames);
        };
        let next = self.segments.get(cursor + 1).map_or(self.frames, |next| next.marker);
        (segment.marker + d - segment.marker * ratio).clamp(0.0, next)
    }

    fn sample(&self, wave: &Wave, d: f64, step: f64, ratio: f64, pitch: f64, mode: Loop, tau: f64, cursor: &mut usize) -> (f32, f32) {
        let end = self.frames * ratio;
        if d >= end || self.segments.is_empty() {
            return (0.0, 0.0);
        }
        let pre = self.rate * PRE_ROLL_SECONDS;
        while let Some(next) = self.segments.get(*cursor + 1)
            && d >= next.marker * ratio - pre
        {
            *cursor += 1;
        }
        let (mut l, mut r) = self.voice(wave, *cursor, d, step, ratio, pitch, mode, tau);
        if *cursor > 0 {
            let marked = self.segments[*cursor].marker * ratio;
            if d < marked {
                let cut = ((marked - d) / pre) as f32;
                let (pl, pr) = self.voice(wave, *cursor - 1, d, step, ratio, pitch, mode, tau);
                l += pl * cut;
                r += pr * cut;
            }
        }
        let closing = ((end - d) / pre).min(1.0) as f32;
        (l * closing, r * closing)
    }

    fn voice(&self, wave: &Wave, k: usize, d: f64, step: f64, ratio: f64, pitch: f64, mode: Loop, tau: f64) -> (f32, f32) {
        let segment = self.segments[k];
        let next = self.segments.get(k + 1).map_or(self.frames, |next| next.marker);
        let pre = self.rate * PRE_ROLL_SECONDS;
        let fade = self.rate * FADE_SECONDS;
        let since = d - segment.marker * ratio;
        let mut gain = if since < 0.0 {
            let t = since + pre;
            let tail = match (mode, k) {
                (Loop::Rest, 1..) => (-(segment.marker - self.segments[k - 1].marker) / tau).exp(),
                _ => 1.0,
            };
            (t / pre * tail).max((t - (pre - fade)) / fade).clamp(0.0, 1.0)
        } else if mode == Loop::Rest {
            (-since / tau).exp()
        } else {
            1.0
        };
        if k == 0 {
            gain *= ((d + 1.0) / fade).min(1.0);
        }
        let mut position = segment.marker + since * pitch;
        if position < 0.0 {
            return (0.0, 0.0);
        }
        match mode {
            Loop::Rest => {
                if position >= next {
                    return (0.0, 0.0);
                }
                gain *= ((next - position) / pre).min(1.0);
            }
            Loop::Forward => {
                let span = segment.turn - segment.mid;
                if position >= segment.turn && span > 0.0 {
                    position = segment.mid + (position - segment.turn) % span;
                    gain *= ((position - segment.mid) / fade).min((segment.turn - position) / fade).min(1.0);
                } else if position >= segment.turn {
                    return (0.0, 0.0);
                }
            }
            Loop::BackAndForth => {
                let span = segment.turn - segment.mid;
                if position >= segment.turn && span > 0.0 {
                    let phase = (position - segment.turn) % (2.0 * span);
                    position = if phase < span { segment.turn - phase } else { segment.mid + (phase - span) };
                } else if position >= segment.turn {
                    return (0.0, 0.0);
                }
            }
        }
        let (l, r) = wave.at(position, step);
        let gain = gain as f32;
        (l * gain, r * gain)
    }
}

#[derive(Clone)]
pub struct Warped {
    pub beats: Arc<Beats>,
    pub ratio: f64,
    pub mode: Loop,
    pub tau: f64,
}

impl PartialEq for Warped {
    fn eq(&self, other: &Warped) -> bool {
        Arc::ptr_eq(&self.beats, &other.beats) && self.ratio == other.ratio && self.mode == other.mode && self.tau == other.tau
    }
}

impl Warped {
    pub fn end(&self) -> f64 {
        self.beats.frames * self.ratio
    }

    pub fn cursor_at(&self, d: f64) -> usize {
        self.beats.cursor_at(d, self.ratio)
    }

    pub fn source_at(&self, d: f64, cursor: usize) -> f64 {
        self.beats.source_at(d, self.ratio, cursor)
    }

    pub fn sample(&self, wave: &Wave, d: f64, step: f64, pitch: f64, cursor: &mut usize) -> (f32, f32) {
        self.beats.sample(wave, d, step, self.ratio, pitch, self.mode, self.tau, cursor)
    }
}

use std::f64::consts::{PI, SQRT_2};

const SPAN_SECONDS: f64 = 431.0 / 48_000.0;
const SETTLED: f64 = 1e-6;
const APART: f64 = 1.001;
const LANES: usize = 8;
pub const MAX_SPLITS: usize = 3;

pub fn half(sample_rate: f64) -> usize {
    (SPAN_SECONDS * sample_rate).round() as usize
}

pub struct FirSplit {
    sample_rate: f64,
    half: usize,
    len: usize,
    taps: Vec<f32>,
    response: Vec<f64>,
    hz: [f64; MAX_SPLITS],
}

impl FirSplit {
    pub fn new(sample_rate: f64) -> FirSplit {
        let half = half(sample_rate);
        let len = 2 * half + 1;
        FirSplit { sample_rate, half, len, taps: vec![0.0; MAX_SPLITS * len], response: vec![0.0; MAX_SPLITS * len], hz: [0.0; MAX_SPLITS] }
    }

    pub fn taps(&self) -> usize {
        self.len
    }

    pub fn half(&self) -> usize {
        self.half
    }

    pub fn design(&mut self, hz: &[f64]) {
        let splits = hz.len().min(MAX_SPLITS);
        let mut apart = [0.0; MAX_SPLITS];
        apart[..splits].copy_from_slice(&hz[..splits]);
        for s in 1..splits {
            apart[s] = apart[s].max(apart[s - 1] * APART);
        }
        let hz = &apart[..splits];
        if hz.iter().zip(&self.hz).all(|(want, have)| (want - have).abs() <= SETTLED * want) {
            return;
        }
        self.hz[..splits].copy_from_slice(hz);
        self.hz[splits..].fill(0.0);
        let (half, len, sr) = (self.half, self.len, self.sample_rate);
        for (s, &fc) in hz.iter().enumerate() {
            let a = 2.0 * PI * fc / SQRT_2;
            let scale = a / (2.0 * sr);
            for i in 0..len {
                let t = (i as f64 - half as f64) / sr;
                let at = a * t.abs();
                self.response[s * len + i] = scale * (-at).exp() * ((a * t).cos() + at.sin());
            }
        }
        let mut fourth = [0.0; MAX_SPLITS];
        for (fourth, hz) in fourth.iter_mut().zip(hz) {
            *fourth = hz.powi(4);
        }
        for b in 0..splits {
            let mut coef = [0.0f64; MAX_SPLITS];
            for mask in 0..(1u32 << b) {
                let members = mask | (1 << b);
                let sign = if mask.count_ones() % 2 == 0 { 1.0 } else { -1.0 };
                for s in (0..=b).filter(|s| members & (1 << s) != 0) {
                    let share = (0..=b).filter(|&k| k != s && members & (1 << k) != 0).fold(sign, |share, k| share * fourth[k] / (fourth[k] - fourth[s]));
                    coef[s] += share;
                }
            }
            for i in 0..len {
                let tap = (0..=b).map(|s| coef[s] * self.response[s * len + i]).sum::<f64>();
                self.taps[b * len + i] = tap as f32;
            }
        }
    }

    pub fn split_each<const W: usize>(&self, windows: [&[f32]; W], bands: [&mut [f32]; W]) {
        match bands.iter().map(|bands| bands.len()).min().unwrap_or(0) {
            0..=1 => self.split_by::<0, W>(windows, bands),
            2 => self.split_by::<1, W>(windows, bands),
            3 => self.split_by::<2, W>(windows, bands),
            _ => self.split_by::<MAX_SPLITS, W>(windows, bands),
        }
    }

    fn split_by<const SPLITS: usize, const W: usize>(&self, windows: [&[f32]; W], bands: [&mut [f32]; W]) {
        let taps = std::array::from_fn(|s| &self.taps[s * self.len..(s + 1) * self.len]);
        for ((bands, ys), window) in bands.into_iter().zip(dots::<SPLITS, W>(taps, windows)).zip(windows) {
            let mut rest = window[self.half];
            for (band, y) in bands.iter_mut().zip(ys) {
                *band = y;
                rest -= y;
            }
            bands[SPLITS] = rest;
        }
    }
}

fn dots<const T: usize, const W: usize>(taps: [&[f32]; T], windows: [&[f32]; W]) -> [[f32; T]; W] {
    let taps = taps.map(<[f32]>::as_chunks::<LANES>);
    let windows = windows.map(<[f32]>::as_chunks::<LANES>);
    let sums = lane_sums(taps.map(|(wide, _)| wide), windows.map(|(wide, _)| wide));
    std::array::from_fn(|w| std::array::from_fn(|t| sums[w][t].iter().sum::<f32>() + taps[t].1.iter().zip(windows[w].1).map(|(x, y)| x * y).sum::<f32>()))
}

#[inline(never)]
fn lane_sums<const T: usize, const W: usize>(taps: [&[[f32; LANES]]; T], windows: [&[[f32; LANES]]; W]) -> [[[f32; LANES]; T]; W] {
    let chunks = taps.iter().chain(&windows).map(|wide| wide.len()).min().unwrap_or(0);
    let taps = taps.map(|taps| &taps[..chunks]);
    let windows = windows.map(|window| &window[..chunks]);
    let mut sums = [[[0.0f32; LANES]; T]; W];
    for chunk in 0..chunks {
        for (sums, window) in sums.iter_mut().zip(windows) {
            for (sums, taps) in sums.iter_mut().zip(taps) {
                for lane in 0..LANES {
                    sums[lane] += taps[chunk][lane] * window[chunk][lane];
                }
            }
        }
    }
    sums
}

pub struct Line {
    buf: Vec<f32>,
    len: usize,
    pos: usize,
}

impl Line {
    pub fn new(len: usize) -> Line {
        Line { buf: vec![0.0; 2 * len], len, pos: 0 }
    }

    pub fn clear(&mut self) {
        self.buf.fill(0.0);
    }

    pub fn push(&mut self, x: f32) -> &[f32] {
        self.pos = (self.pos + 1) % self.len;
        self.buf[self.pos] = x;
        self.buf[self.pos + self.len] = x;
        &self.buf[self.pos + 1..=self.pos + self.len]
    }
}

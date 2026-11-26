pub(crate) mod engine;

use crate::space::engine::{self as space, ALIGN, Analysis, BANDS, Convolver, Designer, Envelope, FILTER, FILTER_BINS, FRAME, Grid, HOP, Kernel, LATENCY, LOOKAHEAD_MAX, Lookahead, Response, Timing};
use sdk::dsp::{Mix, Ring, finite, sanitize};
use sdk::spectrum::CURVE;
use crate::params;
use crate::{Clock, Unit};

const HISTORY: usize = ALIGN + FILTER;
const BLOCKS: usize = 8;
const OUT: usize = BLOCKS * HOP;
const OUT_MASK: usize = OUT - 1;
const Q_SETTLED: f64 = 1e-9;

pub struct Space {
    sample_rate: f64,
    signal_l: Vec<f32>,
    signal_r: Vec<f32>,
    fed: usize,
    read: usize,
    main_l: Ring,
    main_r: Ring,
    signal_ring_l: Ring,
    signal_ring_r: Ring,
    count: usize,
    grid: Grid,
    analysis: Analysis,
    kernel: Kernel,
    kernel_q: f64,
    mask: [f64; BANDS],
    signal_db: [f64; BANDS],
    main_db: [f64; BANDS],
    raw: [f64; BANDS],
    smooth: [f64; BANDS],
    ahead: [f64; BANDS],
    cut: [f64; BANDS],
    lookahead: Lookahead,
    envelope: Envelope,
    frame_l: Vec<f32>,
    frame_r: Vec<f32>,
    signal_frame_l: Vec<f32>,
    signal_frame_r: Vec<f32>,
    segment_l: Vec<f32>,
    segment_r: Vec<f32>,
    cut_bins: Vec<f64>,
    designer: Designer,
    responses: [Response; 2],
    present: [bool; 2],
    current: usize,
    convolver: Convolver,
    block_l: Vec<f64>,
    block_r: Vec<f64>,
    out_l: Vec<f32>,
    out_r: Vec<f32>,
    touched: [bool; BLOCKS],
    mix: Mix,
    hear: Mix,
    shown: [f32; CURVE],
    gauge: f32,
    signal_meter: f32,
}

impl Unit for Space {
    type Params = params::Space;

    fn new(sample_rate: f64, max_frames: usize) -> Space {
        let grid = Grid::new(sample_rate);
        let mut kernel = Kernel::default();
        let q = params::Space::DEFAULT.q;
        grid.kernel(q, &mut kernel);
        Space {
            sample_rate,
            signal_l: vec![0.0; max_frames],
            signal_r: vec![0.0; max_frames],
            fed: 0,
            read: 0,
            main_l: Ring::new(HISTORY),
            main_r: Ring::new(HISTORY),
            signal_ring_l: Ring::new(FRAME),
            signal_ring_r: Ring::new(FRAME),
            count: 0,
            grid,
            analysis: Analysis::default(),
            kernel,
            kernel_q: q,
            mask: [0.0; BANDS],
            signal_db: [0.0; BANDS],
            main_db: [0.0; BANDS],
            raw: [0.0; BANDS],
            smooth: [0.0; BANDS],
            ahead: [0.0; BANDS],
            cut: [0.0; BANDS],
            lookahead: Lookahead::default(),
            envelope: Envelope::default(),
            frame_l: vec![0.0; FRAME],
            frame_r: vec![0.0; FRAME],
            signal_frame_l: vec![0.0; FRAME],
            signal_frame_r: vec![0.0; FRAME],
            segment_l: vec![0.0; FILTER],
            segment_r: vec![0.0; FILTER],
            cut_bins: vec![0.0; FILTER_BINS],
            designer: Designer::default(),
            responses: [Response::default(), Response::default()],
            present: [false; 2],
            current: 0,
            convolver: Convolver::default(),
            block_l: vec![0.0; HOP],
            block_r: vec![0.0; HOP],
            out_l: vec![0.0; OUT],
            out_r: vec![0.0; OUT],
            touched: [false; BLOCKS],
            mix: Mix::new(sample_rate),
            hear: Mix::new(sample_rate),
            shown: [0.0; CURVE],
            gauge: 0.0,
            signal_meter: f32::NEG_INFINITY,
        }
    }

    fn feed(&mut self, l: &[f32], r: &[f32]) {
        let n = l.len().min(r.len()).min(self.signal_l.len());
        self.signal_l[..n].copy_from_slice(&l[..n]);
        self.signal_r[..n].copy_from_slice(&r[..n]);
        self.fed = n;
        self.read = 0;
    }

    fn starve(&mut self) {
        self.fed = 0;
        self.read = 0;
    }

    fn restart(&mut self) {
        for ring in [&mut self.main_l, &mut self.main_r, &mut self.signal_ring_l, &mut self.signal_ring_r] {
            ring.clear();
        }
        self.count = 0;
        self.lookahead.clear();
        self.envelope.clear();
        self.present = [false; 2];
        self.touched = [false; BLOCKS];
        self.mix = Mix::new(self.sample_rate);
        self.hear = Mix::new(self.sample_rate);
        self.shown = [0.0; CURVE];
        self.gauge = 0.0;
        self.signal_meter = f32::NEG_INFINITY;
    }

    fn gauge(&self) -> f32 {
        self.gauge
    }

    fn signal(&self) -> f32 {
        self.signal_meter
    }

    fn curve(&self) -> Option<&[f32; CURVE]> {
        Some(&self.shown)
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], params: &params::Space, wet: f32, _: &Clock) {
        let n = l.len().min(r.len());
        let (l, r) = (&mut l[..n], &mut r[..n]);
        sanitize(l, r);
        let (fed, read) = (self.fed, self.read);
        for i in 0..n {
            let (signal_l, signal_r) = if read + i < fed { (finite(self.signal_l[read + i]), finite(self.signal_r[read + i])) } else { (0.0, 0.0) };
            self.main_l.push(l[i]);
            self.main_r.push(r[i]);
            self.signal_ring_l.push(signal_l);
            self.signal_ring_r.push(signal_r);
            self.count += 1;
            if self.count.is_multiple_of(HOP) {
                self.hop(params);
            }
            let wet = f64::from(self.mix.tick(wet));
            let heard = f64::from(self.hear.tick(if params.listen { 1.0 } else { 0.0 }));
            (l[i], r[i]) = if self.count > LATENCY { self.delayed(wet, heard) } else { (0.0, 0.0) };
        }
        self.read = (read + n).min(fed);
    }
}

impl Space {
    fn delayed(&self, wet: f64, heard: f64) -> (f32, f32) {
        let position = self.count - 1 - LATENCY;
        let (dry_l, dry_r) = (self.main_l.tap(LATENCY + 1), self.main_r.tap(LATENCY + 1));
        let kept = 1.0 - heard;
        if !self.touched[(position / HOP) % BLOCKS] {
            return ((f64::from(dry_l) * kept) as f32, (f64::from(dry_r) * kept) as f32);
        }
        let at = position & OUT_MASK;
        let heard_of = |dry: f32, carved: f32| {
            let removed = wet * (f64::from(dry) - f64::from(carved));
            ((f64::from(dry) - removed) * kept + removed * heard) as f32
        };
        (heard_of(dry_l, self.out_l[at]), heard_of(dry_r, self.out_r[at]))
    }

    fn hop(&mut self, p: &params::Space) {
        let timing = Timing::new(p.attack_ms, p.recovery_ms, self.sample_rate);
        if (p.q - self.kernel_q).abs() > Q_SETTLED {
            self.grid.kernel(p.q, &mut self.kernel);
            self.kernel_q = p.q;
        }
        self.grid.range(p.low_hz, p.high_hz, &mut self.mask);
        for (i, (signal_l, signal_r)) in self.signal_frame_l.iter_mut().zip(&mut self.signal_frame_r).enumerate() {
            *signal_l = self.signal_ring_l.tap(FRAME - i);
            *signal_r = self.signal_ring_r.tap(FRAME - i);
        }
        self.analysis.levels(&self.signal_frame_l, &self.signal_frame_r, &self.grid, &mut self.signal_db);
        if space::signalled(&self.signal_db, &self.mask) {
            for (i, (frame_l, frame_r)) in self.frame_l.iter_mut().zip(&mut self.frame_r).enumerate() {
                *frame_l = self.main_l.tap(FRAME - i);
                *frame_r = self.main_r.tap(FRAME - i);
            }
            self.analysis.levels(&self.frame_l, &self.frame_r, &self.grid, &mut self.main_db);
        }
        self.signal_meter = self.signal_db.iter().copied().fold(f64::NEG_INFINITY, f64::max) as f32;
        space::law(&self.signal_db, &self.main_db, p.depth_db, &self.mask, &mut self.raw);
        self.kernel.apply(&self.raw, &mut self.smooth);
        self.lookahead.push(&self.smooth);
        if self.count / HOP <= LOOKAHEAD_MAX {
            return;
        }
        self.lookahead.ahead(timing.lookahead, &mut self.ahead);
        let deepest = self.envelope.step(&self.ahead, &timing, &mut self.cut);
        self.gauge = deepest as f32;
        self.grid.curve(&self.cut, &mut self.shown);
        if self.count <= ALIGN {
            self.present = [false; 2];
            return;
        }
        let (now, before) = (self.current, 1 - self.current);
        self.present[now] = deepest > 0.0;
        if self.present[now] {
            self.grid.spread(&self.cut, &mut self.cut_bins);
            self.designer.design(&self.cut_bins, &mut self.responses[now]);
        }
        let start = self.count - LATENCY;
        let block = (start / HOP) % BLOCKS;
        if self.present[now] || self.present[before] {
            for (i, (segment_l, segment_r)) in self.segment_l.iter_mut().zip(&mut self.segment_r).enumerate() {
                let tap = ALIGN + FILTER - i;
                *segment_l = self.main_l.tap(tap);
                *segment_r = self.main_r.tap(tap);
            }
            let (present, responses) = (self.present, &self.responses);
            let response = |slot: usize| present[slot].then_some(&responses[slot]);
            self.convolver.block(&self.segment_l, &self.segment_r, response(now), response(before), &mut self.block_l, &mut self.block_r);
            for (i, (block_l, block_r)) in self.block_l.iter().zip(&self.block_r).enumerate() {
                let at = (start + i) & OUT_MASK;
                self.out_l[at] = *block_l as f32;
                self.out_r[at] = *block_r as f32;
            }
        }
        self.touched[block] = self.present[now] || self.present[before];
        self.current = before;
    }
}

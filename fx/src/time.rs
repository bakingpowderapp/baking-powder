use sdk::dsp::{DcBlock, Mix, Ring, lp_coeff, sanitize};
use crate::params;
use crate::{Clock, Unit};
use std::f64::consts::PI;

const HALF: usize = params::Time::KERNEL_HALF;
const LATENCY: usize = params::Time::LATENCY;
const TAPS: usize = 2 * HALF;
const FADE_AT_48K: f64 = 232.0;
const CROSS_AT_48K: f64 = 192.0;
const REFERENCE_RATE: f64 = 48_000.0;
const GRID: usize = 64;
const HIGH_PASS_HZ: f64 = 70.5;
const HAND_BEND: f64 = 1.9;
const LONGEST_LAG_SECONDS: f64 = params::Time::MAX_MS * 0.001 / (params::Time::MIN_CURVE + 1.0);
const WHOLE: f64 = 1e-9;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Motion {
    Rest,
    Stopping,
    Stopped,
    Starting,
    Handing,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fade {
    Off,
    Pending,
    At(usize),
}

impl Fade {
    fn begin(&mut self) {
        if *self == Fade::Pending {
            *self = Fade::At(0);
        }
    }

    fn step(&mut self, over: usize) -> f64 {
        match *self {
            Fade::At(k) => {
                *self = Fade::At(k + 1);
                if k < over { 1.0 - cosine(k as f64 / over as f64) } else { 0.0 }
            }
            Fade::Off | Fade::Pending => 1.0,
        }
    }

    fn advance(&mut self, over: usize) -> Option<f64> {
        match *self {
            Fade::At(k) => {
                *self = Fade::At(k + 1);
                Some((k as f64 / over as f64).min(1.0))
            }
            Fade::Off | Fade::Pending => None,
        }
    }

    fn done(self, over: usize) -> bool {
        matches!(self, Fade::At(k) if k > over)
    }
}

#[derive(Clone, Copy)]
struct Voice {
    lag: f64,
    fresh: bool,
    born_silent: bool,
    silent: bool,
    fade: Fade,
    blockers: [DcBlock; 2],
}

impl Voice {
    fn new(born_silent: bool, silent: bool) -> Voice {
        Voice { lag: 0.0, fresh: true, born_silent, silent, fade: Fade::Off, blockers: [DcBlock::default(); 2] }
    }

    fn advance(&mut self, speed: f64) {
        if self.fresh {
            self.fresh = false;
        } else {
            self.lag += 1.0 - speed;
        }
    }
}

fn cosine(x: f64) -> f64 {
    0.5 * (1.0 - (PI * x).cos())
}

fn bent(x: f64) -> f64 {
    cosine(x.powf(HAND_BEND))
}

fn at(ring: &Ring, delay: usize) -> f32 {
    ring.tap(delay + 1)
}

fn weights(frac: f64) -> [f64; TAPS] {
    let sine = (PI * frac).sin();
    let mut weights = [0.0; TAPS];
    for (k, weight) in weights.iter_mut().enumerate() {
        let x = frac - (k as f64 - (HALF - 1) as f64);
        let sign = if k % 2 == 1 { 1.0 } else { -1.0 };
        *weight = sign * sine / (PI * x) * (0.5 + 0.5 * (PI * x / HALF as f64).cos());
    }
    weights
}

pub struct Time {
    sample_rate: f64,
    ring_l: Ring,
    ring_r: Ring,
    fade_len: usize,
    cross_len: usize,
    mix: Mix,
    motion: Motion,
    seen: Option<bool>,
    u: f64,
    du: f64,
    speed: f64,
    current: Option<Voice>,
    old: Option<Voice>,
    cross: Option<usize>,
    cross_over: usize,
    old_live: f64,
    old_head: f64,
    live_gain: f64,
    head_gain: f64,
    hand: Fade,
    instant: bool,
    clock: u64,
}

impl Time {
    fn read(&self, voice: &Voice) -> (f64, f64) {
        let delay = LATENCY as f64 + voice.lag;
        let whole = delay.floor();
        let frac = delay - whole;
        let base = whole as usize;
        if frac < WHOLE {
            return (f64::from(at(&self.ring_l, base)), f64::from(at(&self.ring_r, base)));
        }
        let (mut l, mut r) = (0.0, 0.0);
        for (k, weight) in weights(frac).into_iter().enumerate() {
            let tap = base + k - (HALF - 1);
            l += f64::from(at(&self.ring_l, tap)) * weight;
            r += f64::from(at(&self.ring_r, tap)) * weight;
        }
        (l, r)
    }

    fn prime(&self, voice: &mut Voice) {
        for (blocker, ring) in voice.blockers.iter_mut().zip([&self.ring_l, &self.ring_r]) {
            if voice.born_silent {
                blocker.prime(at(ring, LATENCY), 0.0);
            } else {
                let before = at(ring, LATENCY + 1);
                blocker.prime(before, before);
            }
        }
    }

    fn voice(&self, voice: &mut Voice, pole: f32, gain: f64) -> (f64, f64) {
        if voice.silent || gain <= 0.0 {
            return (0.0, 0.0);
        }
        let (l, r) = self.read(voice);
        let l = f64::from(voice.blockers[0].tick(l as f32, pole)) * gain;
        let r = f64::from(voice.blockers[1].tick(r as f32, pole)) * gain;
        (l, r)
    }

    fn switch(&mut self, play: bool, curve: f64, du_stop: f64, du_start: f64) {
        let speed = self.speed;
        let from = self.motion;
        match from {
            Motion::Stopping | Motion::Starting | Motion::Handing => {
                self.old = self.current.take();
                self.cross = Some(0);
                (self.old_live, self.old_head) = if from == Motion::Handing { (self.live_gain, self.head_gain) } else { (0.0, 1.0) };
            }
            Motion::Rest | Motion::Stopped => {
                self.old = None;
                self.cross = (from == Motion::Stopped).then_some(0);
                (self.old_live, self.old_head) = (0.0, 1.0);
            }
        }
        self.cross_over = if from == Motion::Stopped { self.fade_len } else { self.cross_len };
        if play {
            self.u = if speed > 0.0 { 1.0 - (1.0 - speed).powf(1.0 / curve) } else { 0.0 };
            self.du = du_start;
            self.motion = Motion::Starting;
        } else {
            self.u = if speed < 1.0 { (1.0 - speed).powf(1.0 / curve) } else { 0.0 };
            self.du = du_stop;
            self.motion = Motion::Stopping;
        }
        self.instant = self.du >= 1.0 / GRID as f64;
        let born_silent = play && from == Motion::Stopped;
        if born_silent && self.instant {
            self.cross = None;
        }
        self.current = Some(Voice::new(born_silent, born_silent && self.instant));
        self.hand = Fade::Off;
    }

    fn moving(&mut self, live: (f32, f32), curve: f64) -> (f64, f64) {
        let Some(mut current) = self.current else {
            return match self.motion {
                Motion::Rest => (f64::from(live.0), f64::from(live.1)),
                _ => (0.0, 0.0),
            };
        };
        let speed = match self.motion {
            Motion::Stopping => (1.0 - self.u.powf(curve)).max(0.0),
            Motion::Starting => 1.0 - (1.0 - self.u).powf(curve),
            Motion::Stopped => 0.0,
            Motion::Rest | Motion::Handing => 1.0,
        };
        if matches!(self.motion, Motion::Stopping | Motion::Starting) && !current.fresh {
            self.u = (self.u + self.du).min(1.0);
        }
        if self.motion == Motion::Stopping && speed <= 0.0 {
            self.motion = Motion::Stopped;
            current.fade = Fade::Pending;
        }
        if self.motion == Motion::Starting && speed >= 1.0 {
            self.motion = Motion::Handing;
            self.hand = Fade::Pending;
        }
        self.speed = speed;
        if current.fresh {
            self.prime(&mut current);
        }
        current.advance(speed);
        let mut old = self.old;
        if let Some(old) = &mut old {
            old.advance(speed);
        }
        if self.clock >= LATENCY as u64 && (self.clock - LATENCY as u64).is_multiple_of(GRID as u64) {
            current.fade.begin();
            if let Some(old) = &mut old {
                old.fade.begin();
            }
            self.hand.begin();
        }
        let (live_share, head_share) = match self.hand.advance(self.fade_len) {
            Some(x) => (if self.instant { cosine(x) } else { bent(x) }, 1.0 - cosine(x)),
            None => (0.0, 1.0),
        };
        let over = self.cross_over;
        let x = match &mut self.cross {
            Some(k) => {
                let x = if *k < over { cosine(*k as f64 / over as f64) } else { 1.0 };
                *k += 1;
                if *k > over {
                    self.cross = None;
                    old = None;
                    (self.old_live, self.old_head) = (0.0, 1.0);
                }
                x
            }
            None => 1.0,
        };
        let pole = lp_coeff(HIGH_PASS_HZ * (1.0 - speed), self.sample_rate);
        let faded = current.fade.step(self.fade_len);
        let (mut l, mut r) = self.voice(&mut current, pole, x * faded * head_share);
        if let Some(old) = &mut old {
            let faded = old.fade.step(self.fade_len);
            let (ol, or) = self.voice(old, pole, (1.0 - x) * self.old_head * faded);
            l += ol;
            r += or;
        }
        let live_gain = live_share + (1.0 - x) * self.old_live;
        l += f64::from(live.0) * live_gain;
        r += f64::from(live.1) * live_gain;
        self.live_gain = live_gain;
        self.head_gain = x * head_share;
        self.old = old;
        self.current = Some(current);
        if self.hand.done(self.fade_len) {
            self.motion = Motion::Rest;
            self.current = None;
            self.old = None;
            self.cross = None;
            self.hand = Fade::Off;
        } else if current.fade.done(self.fade_len) && self.cross.is_none() {
            self.current = None;
            self.old = None;
        }
        (l, r)
    }

    fn tick(&mut self, l: f32, r: f32, curve: f64, wet: f32) -> (f32, f32) {
        self.ring_l.push(l);
        self.ring_r.push(r);
        let live = (at(&self.ring_l, LATENCY), at(&self.ring_r, LATENCY));
        let (wet_l, wet_r) = self.moving(live, curve);
        self.clock += 1;
        let mix = self.mix.tick(wet);
        let dry = 1.0 - mix;
        (wet_l as f32 * mix + live.0 * dry, wet_r as f32 * mix + live.1 * dry)
    }
}

impl Unit for Time {
    type Params = params::Time;

    fn new(sample_rate: f64, _: usize) -> Time {
        let scale = sample_rate / REFERENCE_RATE;
        let fade_len = (FADE_AT_48K * scale).round().max(1.0) as usize;
        let cross_len = (CROSS_AT_48K * scale).round().max(1.0) as usize;
        let ring = (LONGEST_LAG_SECONDS * sample_rate).ceil() as usize + TAPS + GRID + fade_len + cross_len;
        Time {
            sample_rate,
            ring_l: Ring::new(ring),
            ring_r: Ring::new(ring),
            fade_len,
            cross_len,
            mix: Mix::new(sample_rate),
            motion: Motion::Rest,
            seen: None,
            u: 0.0,
            du: 0.0,
            speed: 1.0,
            current: None,
            old: None,
            cross: None,
            cross_over: cross_len,
            old_live: 0.0,
            old_head: 1.0,
            live_gain: 1.0,
            head_gain: 1.0,
            hand: Fade::Off,
            instant: false,
            clock: 0,
        }
    }

    fn restart(&mut self) {
        self.ring_l.clear();
        self.ring_r.clear();
        self.motion = Motion::Rest;
        self.seen = None;
        self.u = 0.0;
        self.speed = 1.0;
        self.current = None;
        self.old = None;
        self.cross = None;
        self.old_live = 0.0;
        self.old_head = 1.0;
        self.live_gain = 1.0;
        self.head_gain = 1.0;
        self.hand = Fade::Off;
        self.clock = 0;
    }

    fn gauge(&self) -> f32 {
        self.speed as f32
    }

    fn signal(&self) -> f32 {
        match self.motion {
            Motion::Rest => f32::NEG_INFINITY,
            Motion::Stopping | Motion::Stopped => 0.0,
            Motion::Starting | Motion::Handing => 1.0,
        }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], p: &params::Time, wet: f32, _: &Clock) {
        let n = l.len().min(r.len());
        sanitize(l, r);
        let frames_per_ms = self.sample_rate / 1000.0;
        let du_stop = if p.stop_ms > 0.0 { 1.0 / (p.stop_ms * frames_per_ms) } else { f64::INFINITY };
        let du_start = 1.0 / (p.start_ms * frames_per_ms).max(GRID as f64);
        match self.seen {
            None => {
                self.seen = Some(p.play);
                if !p.play {
                    self.motion = Motion::Stopped;
                    self.speed = 0.0;
                }
            }
            Some(was) if was != p.play => {
                self.switch(p.play, p.curve, du_stop, du_start);
                self.seen = Some(p.play);
            }
            Some(_) => {}
        }
        for i in 0..n {
            (l[i], r[i]) = self.tick(l[i], r[i], p.curve, wet);
        }
    }
}

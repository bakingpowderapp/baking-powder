#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Phase {
    #[default]
    Idle,
    Delay,
    Attack,
    Hold,
    Decay,
    Sustain,
    Release,
}

pub const SHARPEST: f64 = 8.0;
pub const FALL: f64 = -0.25;
const STRAIGHT: f64 = 1e-3;
const LEAST: f64 = 1e-6;
const HALVINGS: usize = 40;
const DELAY: usize = 0;
pub const ATTACK: usize = 1;
const HOLD: usize = 2;
pub const DECAY: usize = 3;
pub const RELEASE: usize = 4;
const ATTACK_BEND: usize = 0;
const DECAY_BEND: usize = 1;
const RELEASE_BEND: usize = 2;

fn frames(seconds: f32, sample_rate: f32) -> u32 {
    (seconds * sample_rate).round().max(0.0) as u32
}

fn sharpness(bend: f64) -> f64 {
    SHARPEST * bend.abs().min(1.0)
}

fn early(bend: f64, rising: bool) -> bool {
    (bend > 0.0) == rising
}

fn signed(k: f64, early: bool) -> f64 {
    if early { -k } else { k }
}

fn ease(u: f64, k: f64, early: bool) -> f64 {
    if k < STRAIGHT {
        return u;
    }
    let s = signed(k, early);
    ((s * u).exp() - 1.0) / (s.exp() - 1.0)
}

fn progress(reached: f64, k: f64, early: bool) -> f64 {
    if k < STRAIGHT {
        return reached;
    }
    let s = signed(k, early);
    (1.0 + reached * (s.exp() - 1.0)).ln() / s
}

pub fn level(from: f64, to: f64, bend: f64, u: f64) -> f64 {
    from + (to - from) * ease(u.clamp(0.0, 1.0), sharpness(bend), early(bend, to > from))
}

pub fn bend_through(from: f64, to: f64, u: f64, level: f64) -> Option<f64> {
    let rise = to - from;
    if rise == 0.0 || u <= 0.0 || u >= 1.0 {
        return None;
    }
    let reached = ((level - from) / rise).clamp(LEAST, 1.0 - LEAST);
    let early = reached > u;
    let (u, reached) = if early { (u, reached) } else { (1.0 - u, 1.0 - reached) };
    let (mut lo, mut hi) = (0.0, SHARPEST);
    if ease(u, hi, true) > reached {
        for _ in 0..HALVINGS {
            let mid = 0.5 * (lo + hi);
            if ease(u, mid, true) < reached {
                lo = mid;
            } else {
                hi = mid;
            }
        }
    }
    let bend = hi / SHARPEST;
    Some(if early == (rise > 0.0) { bend } else { -bend })
}

#[derive(Clone, Copy, Debug, Default)]
struct Run {
    u: f64,
    du: f64,
    s: f64,
    g: f64,
    r: f64,
    norm: f64,
}

impl Run {
    fn enter(u: f64, seconds: f32, bend: f64, rising: bool, sample_rate: f32) -> Run {
        let mut run = Run { u, ..Run::default() };
        run.tune(seconds, bend, rising, sample_rate);
        run
    }

    fn tune(&mut self, seconds: f32, bend: f64, rising: bool, sample_rate: f32) {
        let du = 1.0 / f64::from((seconds * sample_rate).max(1.0));
        let k = sharpness(bend);
        let s = if k < STRAIGHT { 0.0 } else { signed(k, early(bend, rising)) };
        if (du, s) == (self.du, self.s) {
            return;
        }
        self.du = du;
        self.s = s;
        if s != 0.0 {
            self.g = (s * self.u).exp();
            self.r = (s * du).exp();
            self.norm = 1.0 / (s.exp() - 1.0);
        }
    }

    fn tick(&mut self) -> f64 {
        self.u += self.du;
        if self.u >= 1.0 {
            self.u = 1.0;
            return 1.0;
        }
        if self.s == 0.0 {
            return self.u;
        }
        self.g *= self.r;
        (self.g - 1.0) * self.norm
    }

    fn done(&self) -> bool {
        self.u >= 1.0
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Adsr {
    pub phase: Phase,
    value: f64,
    from: f64,
    top: f64,
    run: Run,
    times: [f32; 5],
    bends: [f64; 3],
    sustain: f64,
    left: u32,
    sample_rate: f32,
}

impl Default for Adsr {
    fn default() -> Adsr {
        Adsr { phase: Phase::Idle, value: 0.0, from: 0.0, top: 1.0, run: Run::default(), times: [0.0; 5], bends: [0.0, FALL, FALL], sustain: 0.8, left: 0, sample_rate: 48_000.0 }
    }
}

impl Adsr {
    pub fn set_times(&mut self, times: [f32; 5], bends: [f64; 3], sustain: f64, sample_rate: f32) {
        self.times = times;
        self.bends = bends;
        self.sustain = sustain.clamp(0.0, 1.0);
        self.sample_rate = sample_rate;
        match self.phase {
            Phase::Attack => self.run.tune(times[ATTACK], bends[ATTACK_BEND], true, sample_rate),
            Phase::Decay => self.run.tune(times[DECAY], bends[DECAY_BEND], false, sample_rate),
            Phase::Release => self.run.tune(times[RELEASE], bends[RELEASE_BEND], false, sample_rate),
            Phase::Idle | Phase::Delay | Phase::Hold | Phase::Sustain => {}
        }
    }

    pub fn trigger(&mut self) {
        let delay = frames(self.times[DELAY], self.sample_rate);
        if delay > 0 {
            self.phase = Phase::Delay;
            self.left = delay;
            return;
        }
        self.attack();
    }

    pub fn rescale(&mut self, factor: f64) {
        self.value *= factor;
    }

    fn attack(&mut self) {
        if self.value > 1.0 {
            self.phase = Phase::Decay;
            self.top = self.value;
            self.run = Run::enter(0.0, self.times[DECAY], self.bends[DECAY_BEND], false, self.sample_rate);
            return;
        }
        self.phase = Phase::Attack;
        let bend = self.bends[ATTACK_BEND];
        let u = progress(self.value.clamp(0.0, 1.0), sharpness(bend), early(bend, true));
        self.run = Run::enter(u, self.times[ATTACK], bend, true, self.sample_rate);
    }

    fn decay(&mut self) {
        self.phase = Phase::Decay;
        self.top = 1.0;
        self.run = Run::enter(0.0, self.times[DECAY], self.bends[DECAY_BEND], false, self.sample_rate);
    }

    pub fn release(&mut self) {
        if self.phase == Phase::Idle {
            return;
        }
        if self.value <= 0.0 {
            self.phase = Phase::Idle;
            return;
        }
        self.fall();
    }

    fn fall(&mut self) {
        self.phase = Phase::Release;
        self.from = self.value;
        self.run = Run::enter(0.0, self.times[RELEASE], self.bends[RELEASE_BEND], false, self.sample_rate);
    }

    fn peak(&mut self) {
        let hold = frames(self.times[HOLD], self.sample_rate);
        if hold > 0 {
            self.phase = Phase::Hold;
            self.left = hold;
        } else {
            self.decay();
        }
    }

    pub fn rest(&mut self) {
        self.phase = Phase::Idle;
        self.value = 0.0;
    }

    pub fn value(&self) -> f64 {
        self.value
    }

    pub fn tick(&mut self) -> f64 {
        match self.phase {
            Phase::Idle => 0.0,
            Phase::Delay => {
                self.left -= 1;
                if self.left == 0 {
                    self.attack();
                    return self.tick();
                }
                self.value
            }
            Phase::Attack => {
                self.value = self.run.tick();
                if self.run.done() {
                    self.peak();
                }
                self.value
            }
            Phase::Hold => {
                self.value = 1.0;
                self.left -= 1;
                if self.left == 0 {
                    self.decay();
                }
                self.value
            }
            Phase::Decay => {
                self.value = self.top + (self.sustain - self.top) * self.run.tick();
                if self.run.done() {
                    self.phase = Phase::Sustain;
                }
                self.value
            }

            Phase::Sustain => {
                self.value = self.sustain;
                self.value
            }
            Phase::Release => {
                self.value = self.from * (1.0 - self.run.tick());
                if self.run.done() {
                    self.phase = Phase::Idle;
                }
                self.value
            }
        }
    }
}


#[derive(Clone, Copy, Debug)]
pub struct Smoother {
    pub z: f32,
    coeff: f32,
}

impl Smoother {
    pub const SECONDS: f32 = 0.030;
    pub const FAST_SECONDS: f32 = 0.005;

    pub fn new(initial: f32, sample_rate: f32) -> Smoother {
        Smoother::over(initial, sample_rate, Smoother::SECONDS)
    }

    pub fn fast(initial: f32, sample_rate: f32) -> Smoother {
        Smoother::over(initial, sample_rate, Smoother::FAST_SECONDS)
    }

    fn over(initial: f32, sample_rate: f32, seconds: f32) -> Smoother {
        Smoother { z: initial, coeff: smoothing_coefficient(seconds, sample_rate) }
    }

    pub fn tick(&mut self, target: f32) -> f32 {
        self.z += self.coeff * (target - self.z);
        self.z
    }
}

fn smoothing_coefficient(seconds: f32, sample_rate: f32) -> f32 {
    1.0 - (-1.0 / (seconds * sample_rate)).exp()
}

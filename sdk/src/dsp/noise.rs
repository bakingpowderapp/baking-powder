use crate::schema::choice;
use crate::dsp::svf::Svf;
use std::f64::consts::PI;

choice!(pub NoiseColor { White = "White" / "white", Pink = "Pink" / "pink", Brown = "Brown" / "brown" });

const WORD: usize = 32;
const XORSHIFT_PERIOD: u64 = u32::MAX as u64;
static LEAPS: [[u32; WORD]; WORD] = leaps();

const fn stepped(mut x: u32) -> u32 {
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    x
}

const fn leapt(leap: &[u32; WORD], x: u32) -> u32 {
    let (mut out, mut bit) = (0, 0);
    while bit < WORD {
        out ^= leap[bit] & ((x >> bit) & 1).wrapping_neg();
        bit += 1;
    }
    out
}

const fn leaps() -> [[u32; WORD]; WORD] {
    let mut leaps = [[0; WORD]; WORD];
    let mut bit = 0;
    while bit < WORD {
        leaps[0][bit] = stepped(1 << bit);
        bit += 1;
    }
    let mut doubling = 1;
    while doubling < WORD {
        let mut bit = 0;
        while bit < WORD {
            leaps[doubling][bit] = leapt(&leaps[doubling - 1], leaps[doubling - 1][bit]);
            bit += 1;
        }
        doubling += 1;
    }
    leaps
}

pub fn xorshift32(state: &mut u32) -> u32 {
    *state = stepped(*state);
    *state
}

pub fn xorshift32_after(state: u32, steps: u64) -> u32 {
    let mut left = steps % XORSHIFT_PERIOD;
    let (mut x, mut doubling) = (state, 0);
    while left != 0 {
        if left & 1 == 1 {
            x = leapt(&LEAPS[doubling], x);
        }
        left >>= 1;
        doubling += 1;
    }
    x
}

pub fn white_of(drawn: u32) -> f64 {
    f64::from(drawn) / 4294967296.0 * 2.0 - 1.0
}

pub fn white(state: &mut u32) -> f64 {
    white_of(xorshift32(state))
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Stereo {
    pub l: f64,
    pub r: f64,
}

#[derive(Clone, Copy, Debug, Default)]
struct Pink {
    p: [f64; 7],
}

impl Pink {
    fn tick(&mut self, white: f64) -> f64 {
        let p = &mut self.p;
        p[0] = 0.99886 * p[0] + white * 0.0555179;
        p[1] = 0.99332 * p[1] + white * 0.0750759;
        p[2] = 0.96900 * p[2] + white * 0.1538520;
        p[3] = 0.86650 * p[3] + white * 0.3104856;
        p[4] = 0.55000 * p[4] + white * 0.5329522;
        p[5] = -0.7616 * p[5] - white * 0.0168980;
        let pink = p[0] + p[1] + p[2] + p[3] + p[4] + p[5] + p[6] + white * 0.5362;
        p[6] = white * 0.115926;
        pink
    }

    fn flush_denormals(&mut self) {
        for v in &mut self.p {
            *v += 1e-25;
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Brown {
    integrator: f64,
    dc_x1: f64,
    dc_y1: f64,
}

impl Brown {
    fn tick(&mut self, white: f64, a: f64, b: f64, r: f64) -> f64 {
        self.integrator = a * self.integrator + b * white;
        let z = self.integrator - self.dc_x1 + r * self.dc_y1;
        self.dc_x1 = self.integrator;
        self.dc_y1 = z;
        z
    }

    fn flush_denormals(&mut self) {
        self.integrator += 1e-25;
        self.dc_x1 += 1e-25;
        self.dc_y1 += 1e-25;
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Channel {
    prng: u32,
    pink: Pink,
    brown: Brown,
    tone: Svf,
}

impl Channel {
    fn flush_denormals(&mut self) {
        self.pink.flush_denormals();
        self.brown.flush_denormals();
        self.tone.s1 += 1e-25;
        self.tone.s2 += 1e-25;
    }
}

#[derive(Clone, Debug)]
pub struct Noise {
    center: Channel,
    side: Channel,
    color: NoiseColor,
    width: f64,
    tone_active: bool,
    tone_is_highpass: bool,
    tone_g: f64,
    sample_rate: f64,
    brown_a: f64,
    brown_b: f64,
    brown_r: f64,
}

const CENTER_SEED: u32 = 0xDEAD_BEEF;
const SIDE_SEED: u32 = 0xCAFE_F00D;
const WHITE_TRIM: f64 = 1.0;
const PINK_TRIM: f64 = 0.490335;
const BROWN_TRIM: f64 = 3.160122;
const TONE_Q: f64 = 0.707;
const TONE_DAMPING: f64 = 1.0 / TONE_Q;
const TONE_MIN_HZ: f64 = 20.0;
const TONE_MAX_OCTAVES: f64 = 10.0;

impl Default for Noise {
    fn default() -> Noise {
        let mut noise = Noise {
            center: Channel { prng: CENTER_SEED, ..Channel::default() },
            side: Channel { prng: SIDE_SEED, ..Channel::default() },
            color: NoiseColor::White,
            width: 0.0,
            tone_active: false,
            tone_is_highpass: false,
            tone_g: 0.0,
            sample_rate: 0.0,
            brown_a: 0.0,
            brown_b: 0.0,
            brown_r: 0.0,
        };
        noise.set_sample_rate(48000.0);
        noise
    }
}

impl Noise {
    pub fn seed(&mut self, s1: u32, s2: u32) {
        self.center.prng = if s1 == 0 { CENTER_SEED } else { s1 };
        self.side.prng = if s2 == 0 { SIDE_SEED } else { s2 };
    }

    pub fn set_color(&mut self, color: NoiseColor) {
        self.color = color;
    }

    pub fn set_tone_norm(&mut self, n: f64) {
        if n.abs() < 0.01 {
            self.tone_active = false;
            return;
        }
        self.tone_active = true;
        self.tone_is_highpass = n < 0.0;
        let octaves = if n < 0.0 { -n } else { 1.0 - n } * TONE_MAX_OCTAVES;
        self.tone_g = self.tan_g_for_corner(TONE_MIN_HZ * octaves.exp2());
    }

    pub fn set_width(&mut self, w: f64) {
        self.width = w.clamp(0.0, 1.0);
    }

    pub fn set_sample_rate(&mut self, sample_rate: f64) {
        if sample_rate == self.sample_rate {
            return;
        }
        self.sample_rate = sample_rate;
        self.brown_a = (-2.0 * PI * 10.0 / sample_rate).exp();
        self.brown_b = (1.0 - self.brown_a * self.brown_a).sqrt();
        self.brown_r = (-2.0 * PI * 2.0 / sample_rate).exp();
    }

    pub fn flush_denormals(&mut self) {
        self.center.flush_denormals();
        self.side.flush_denormals();
    }

    pub fn tick(&mut self) -> Stereo {
        let primary = self.colored_and_shaped(false);
        if self.width <= 0.0 {
            return Stereo { l: primary, r: primary };
        }
        let secondary = self.colored_and_shaped(true);
        let theta = self.width * (PI / 4.0);
        let (c, s) = (theta.cos(), theta.sin());
        Stereo { l: primary * c - secondary * s, r: primary * c + secondary * s }
    }

    fn colored_and_shaped(&mut self, side: bool) -> f64 {
        let (brown_a, brown_b, brown_r) = (self.brown_a, self.brown_b, self.brown_r);
        let color = self.color;
        let channel = if side { &mut self.side } else { &mut self.center };
        let w = white(&mut channel.prng);
        let mut s = match color {
            NoiseColor::Pink => channel.pink.tick(w) * PINK_TRIM,
            NoiseColor::Brown => channel.brown.tick(w, brown_a, brown_b, brown_r) * BROWN_TRIM,
            NoiseColor::White => w * WHITE_TRIM,
        };
        if self.tone_active {
            s = self.tone(side, s);
        }
        s
    }

    fn tone(&mut self, side: bool, x: f64) -> f64 {
        let (g, highpass) = (self.tone_g, self.tone_is_highpass);
        let channel = if side { &mut self.side } else { &mut self.center };
        let taps = channel.tone.tick(x, g, TONE_DAMPING);
        if highpass { taps.hp } else { taps.lp }
    }

    fn tan_g_for_corner(&self, fc: f64) -> f64 {
        let f = fc.clamp(1.0, self.sample_rate * 0.49);
        (PI * f / self.sample_rate).tan()
    }
}

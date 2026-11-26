use crate::dsp::Halfband;

#[derive(Clone, Copy, Debug, Default)]
pub struct Taps {
    pub lp6: f32,
    pub lp12: f32,
    pub lp18: f32,
    pub lp24: f32,
}

impl Taps {
    pub fn bp12(self) -> f32 {
        2.0 * (self.lp6 - self.lp12)
    }

    pub fn bp24(self) -> f32 {
        4.0 * (self.lp12 - 2.0 * self.lp18 + self.lp24)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Ladder {
    s1: f32,
    s2: f32,
    s3: f32,
    s4: f32,
    up: Halfband,
    down6: Halfband,
    down12: Halfband,
    down18: Halfband,
    down24: Halfband,
    back: f32,
}

const DRIVE_ENGAGE: f32 = 1e-4;

impl Ladder {
    pub fn tick(&mut self, x: f32, g: f32, k: f32, drive: f32) -> Taps {
        let g_agg = g * g * g * g;
        let s_agg = (1.0 - g) * (g * g * g * self.s1 + g * g * self.s2 + g * self.s3 + self.s4);
        let driven = drive > DRIVE_ENGAGE;
        let a = if driven { 1.0 / drive } else { 1.0 };
        let xd = if driven {
            let g_d = 1.0 + 4.0 * drive;
            a * (g_d * x / a).tanh()
        } else {
            x
        };
        let mut u = (xd - k * s_agg) / (1.0 + k * g_agg);
        if k != 0.0 {
            for _ in 0..2 {
                let t = (g_agg * u + s_agg).tanh();
                let f = u - xd + k * t;
                let fp = 1.0 + k * g_agg * (1.0 - t * t);
                u -= f / fp;
            }
        }
        let shape = |v: f32| if driven { a * (v / a).tanh() } else { v };
        let stage = |s: &mut f32, input: f32| {
            let v = (shape(input) - *s) * g;
            let t = v + *s;
            *s = t + v;
            t
        };
        let y1 = stage(&mut self.s1, u);
        let y2 = stage(&mut self.s2, y1);
        let y3 = stage(&mut self.s3, y2);
        let y4 = stage(&mut self.s4, y3);
        if !y4.is_finite() {
            self.s1 = 0.0;
            self.s2 = 0.0;
            self.s3 = 0.0;
            self.s4 = 0.0;
            return Taps::default();
        }
        Taps { lp6: y1, lp12: y2, lp18: y3, lp24: y4 }
    }

    pub fn tick2x_lanes<const N: usize>(lanes: &mut [Ladder], x: [f32; N], g: f32, k: f32, drive: f32, loop_gain: f32, tap: fn(Taps) -> f32) -> [Taps; N] {
        let fed = |u: f32, back: f32| if loop_gain == 0.0 { u } else { u + loop_gain * back };
        let up: [(f32, f32); N] = std::array::from_fn(|lane| {
            let risen = lanes[lane].up.upsample(x[lane]);
            if risen.0.is_finite() && risen.1.is_finite() {
                return risen;
            }
            lanes[lane].reset();
            (0.0, 0.0)
        });
        let a: [Taps; N] = std::array::from_fn(|lane| {
            let ladder = &mut lanes[lane];
            ladder.tick(fed(up[lane].0, ladder.back), g, k, drive)
        });
        for (lane, taps) in a.into_iter().enumerate() {
            lanes[lane].back = tap(taps);
        }
        let b: [Taps; N] = std::array::from_fn(|lane| {
            let ladder = &mut lanes[lane];
            ladder.tick(fed(up[lane].1, ladder.back), g, k, drive)
        });
        for (lane, taps) in b.into_iter().enumerate() {
            lanes[lane].back = tap(taps);
        }
        std::array::from_fn(|lane| {
            let ladder = &mut lanes[lane];
            Taps { lp6: ladder.down6.downsample(a[lane].lp6, b[lane].lp6), lp12: ladder.down12.downsample(a[lane].lp12, b[lane].lp12), lp18: ladder.down18.downsample(a[lane].lp18, b[lane].lp18), lp24: ladder.down24.downsample(a[lane].lp24, b[lane].lp24) }
        })
    }

    pub fn reset(&mut self) {
        *self = Ladder::default();
    }
}

pub fn ladder_feedback(resonance: f32) -> f32 {
    let r = resonance.clamp(0.0, 1.0);
    let knee = 0.92;
    let k_knee = 3.9;
    let k_max = 4.2;
    if r < knee {
        return k_knee * (r / knee).powf(1.6);
    }
    let t = (r - knee) / (1.0 - knee);
    k_knee + (k_max - k_knee) * (t * t * (3.0 - 2.0 * t))
}

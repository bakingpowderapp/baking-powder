use std::f64::consts::PI;

pub const MAX_UNISON: usize = 16;
const PAN_SPREAD: f64 = 1.0;
const DETUNE_WARP: f64 = 1.5;
const WIDTH_TAPER: f64 = 1.4;
const MAX_SIDE_MID_DB: f64 = -6.0;
const CENTRE_LIFT: f64 = 1.063;

#[derive(Clone, Copy, Debug)]
pub struct Spread {
    pub detune: [f64; MAX_UNISON],
    pub gain_l: [f64; MAX_UNISON],
    pub gain_r: [f64; MAX_UNISON],
}

impl Default for Spread {
    fn default() -> Spread {
        Spread { detune: [1.0; MAX_UNISON], gain_l: [1.0; MAX_UNISON], gain_r: [1.0; MAX_UNISON] }
    }
}

fn build(n: usize) -> f64 {
    let build_db = (4.0 * ((n - 1) as f64 / 7.0).sqrt()).min(6.5);
    (build_db * (10f64.ln() / 20.0)).exp()
}

impl Spread {
    pub fn fill(&mut self, count: usize, detune_cents: f64, width: f64, blend: f64) {
        let n = count.clamp(1, MAX_UNISON);
        if n == 1 {
            self.detune[0] = 1.0;
            self.gain_l[0] = 1.0;
            self.gain_r[0] = 1.0;
            return;
        }
        let center_count = if n % 2 == 1 { 1 } else { 2 };
        let side = blend.clamp(0.0, 1.0);
        let sum_a2 = center_count as f64 * CENTRE_LIFT * CENTRE_LIFT + (n - center_count) as f64 * side * side;
        let comp = build(n) / sum_a2.sqrt();
        let width_amt = width.clamp(0.0, 1.0).powf(WIDTH_TAPER) * PAN_SPREAD;
        let mid = n / 2;
        for u in 0..n {
            let t = (u as f64 / (n - 1) as f64) * 2.0 - 1.0;
            let s = t.signum() * t.abs().powf(DETUNE_WARP);
            self.detune[u] = (s * detune_cents / 1200.0).exp2();
            let is_center = if n % 2 == 1 { u == mid } else { u == mid - 1 || u == mid };
            self.gain_l[u] = (if is_center { CENTRE_LIFT } else { side }) * comp;
            self.gain_r[u] = (s * width_amt).clamp(-1.0, 1.0);
        }
        let cap_ratio = 10f64.powf(MAX_SIDE_MID_DB / 10.0);
        let mut k = 1.0;
        let (mut side1, mut mid1) = (0.0, 0.0);
        for u in 0..n {
            let a2 = self.gain_l[u] * self.gain_l[u];
            let c = (self.gain_r[u] * PI / 2.0).cos();
            mid1 += a2 * (1.0 + c);
            side1 += a2 * (1.0 - c);
        }
        if mid1 > 0.0 && side1 > cap_ratio * mid1 {
            let (mut lo, mut hi) = (0.0, 1.0);
            for _ in 0..24 {
                let m = 0.5 * (lo + hi);
                let (mut side, mut mid_e) = (0.0, 0.0);
                for u in 0..n {
                    let a2 = self.gain_l[u] * self.gain_l[u];
                    let c = (m * self.gain_r[u] * PI / 2.0).cos();
                    mid_e += a2 * (1.0 + c);
                    side += a2 * (1.0 - c);
                }
                if side > cap_ratio * mid_e {
                    hi = m;
                } else {
                    lo = m;
                }
            }
            k = lo;
        }
        for u in 0..n {
            let amp = self.gain_l[u];
            let pan = 0.5 + 0.5 * k * self.gain_r[u];
            self.gain_l[u] = amp * 2f64.sqrt() * (pan * PI / 2.0).cos();
            self.gain_r[u] = amp * 2f64.sqrt() * (pan * PI / 2.0).sin();
        }
    }

    pub fn alternate(&mut self, count: usize, detune_cents: f64, width: f64, blend: f64) {
        let n = count.clamp(1, MAX_UNISON);
        if n == 1 {
            return self.fill(n, detune_cents, width, blend);
        }
        let reach = width.clamp(0.0, 1.0);
        let middle = (n % 2 == 1).then_some(n / 2);
        let outer = blend.clamp(0.0, 1.0);
        let inner = |u: usize| match middle {
            Some(middle) => u == middle,
            None => u == n / 2 - 1 || u == n / 2,
        };
        let shares = if middle.is_some() { 0.5 + (n - 1) as f64 * outer * outer } else { 2.0 + (n - 2) as f64 * outer * outer };
        let stack = build(n) / shares.sqrt();
        for u in 0..n {
            let t = (u as f64 / (n - 1) as f64) * 2.0 - 1.0;
            self.detune[u] = (t * detune_cents / 1200.0).exp2();
            let amp = if inner(u) { stack } else { stack * outer };
            if middle == Some(u) {
                self.gain_l[u] = amp / 2f64.sqrt();
                self.gain_r[u] = amp / 2f64.sqrt();
                continue;
            }
            let turn = u - usize::from(middle.is_some_and(|middle| u > middle));
            let side = if turn.is_multiple_of(2) { -reach } else { reach };
            let pan = 0.5 + 0.5 * side;
            self.gain_l[u] = amp * 2f64.sqrt() * (pan * PI / 2.0).cos();
            self.gain_r[u] = amp * 2f64.sqrt() * (pan * PI / 2.0).sin();
        }
    }
}
